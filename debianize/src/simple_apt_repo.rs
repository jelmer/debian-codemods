use flate2::write::GzEncoder;
use flate2::Compression;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, StatusCode};
use std::fs;
use std::io;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};

pub struct SimpleTrustedAptRepo {
    directory: PathBuf,
    server_addr: Arc<Mutex<Option<SocketAddr>>>,
    thread: Option<JoinHandle<()>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<String>>,
}

impl SimpleTrustedAptRepo {
    pub fn new(directory: PathBuf) -> Self {
        SimpleTrustedAptRepo {
            directory,
            server_addr: Arc::new(Mutex::new(None)),
            thread: None,
            shutdown_tx: None,
        }
    }

    pub fn url(&self) -> Option<url::Url> {
        if let Some(addr) = self.server_addr.lock().unwrap().as_ref() {
            url::Url::parse(&format!("http://{}:{}/", addr.ip(), addr.port())).ok()
        } else {
            None
        }
    }

    pub fn start(&mut self) -> io::Result<()> {
        if self.thread.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "server already active",
            ));
        }

        let directory = Arc::new(self.directory.clone());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<String>();
        self.shutdown_tx = Some(shutdown_tx);
        let (start_tx, start_rx) = mpsc::channel::<SocketAddr>();
        let server_addr = Arc::clone(&self.server_addr);

        let make_svc = make_service_fn(move |_conn| {
            let directory = Arc::clone(&directory);
            async move {
                Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| {
                    let directory = Arc::clone(&directory);
                    async move {
                        let path = directory.join(req.uri().path().trim_start_matches('/'));
                        match fs::read(path) {
                            Ok(contents) => {
                                Ok::<_, hyper::Error>(Response::new(Body::from(contents)))
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                                Ok(Response::builder()
                                    .status(StatusCode::NOT_FOUND)
                                    .body(Body::from("File not found"))
                                    .unwrap())
                            }
                            Err(e) => {
                                log::error!("Error reading file: {}", e);
                                Ok(Response::builder()
                                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                                    .body(Body::from("Internal server error"))
                                    .unwrap())
                            }
                        }
                    }
                }))
            }
        });

        let addr = SocketAddr::from(([127, 0, 0, 1], 0));

        let handle = thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let server = Server::bind(&addr).serve(make_svc);
                let bound_addr = server.local_addr();
                let server_future = server.with_graceful_shutdown(async {
                    shutdown_rx.await.ok();
                });
                *server_addr.lock().unwrap() = Some(bound_addr);
                start_tx.send(bound_addr).unwrap();

                if let Err(e) = server_future.await {
                    log::error!("server error: {}", e);
                }
            });
        });

        let server_addr = start_rx.recv().unwrap();

        log::info!(
            "Local apt repo started at http://{}:{}/",
            server_addr.ip(),
            server_addr.port()
        );

        self.thread = Some(handle);

        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            shutdown_tx.send("shutdown".to_string()).unwrap();
        }
        if let Some(handle) = self.thread.take() {
            // This will stop the server
            handle.join().unwrap();
        }
        *self.server_addr.lock().unwrap() = None;
    }

    pub fn sources_lines(&self) -> Vec<String> {
        let server_addr = self.server_addr.lock().unwrap();
        if server_addr.is_none() {
            return vec![];
        }
        let packages_path = Path::new(&self.directory).join("Packages.gz");
        if packages_path.exists() {
            let addr = server_addr.unwrap();
            vec![format!(
                "deb [trusted=yes] http://{}:{}/ ./",
                addr.ip(),
                addr.port()
            )]
        } else {
            vec![]
        }
    }

    /// Refresh the repository metadata
    ///
    /// This method runs `dpkg-scanpackages` to generate the `Packages.gz` file.
    pub fn refresh(&self) -> io::Result<()> {
        let output = Command::new("dpkg-scanpackages")
            .arg("-m")
            .arg(".")
            .arg("/dev/null")
            .current_dir(&self.directory)
            .output()?;

        if output.status.success() {
            let packages_path = Path::new(&self.directory).join("Packages.gz");
            let file = fs::File::create(packages_path)?;
            let mut encoder = GzEncoder::new(file, Compression::default());
            encoder.write_all(&output.stdout)?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Failed to run dpkg-scanpackages",
            ));
        }

        Ok(())
    }
}

impl Drop for SimpleTrustedAptRepo {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    #[test]
    fn test_simple() {
        let td = tempfile::tempdir().unwrap();
        let mut repo = SimpleTrustedAptRepo::new(td.path().to_path_buf());

        let sources_lines = repo.sources_lines();
        assert_eq!(sources_lines.len(), 0);

        // Start the server
        repo.start().unwrap();

        let sources_lines = repo.sources_lines();
        assert_eq!(sources_lines.len(), 0);

        let file = fs::File::create(td.path().join("Packages.gz")).unwrap();
        let mut encoder = GzEncoder::new(file, Compression::default());
        encoder.write_all(b"Hello, world!").unwrap();
        encoder.finish().unwrap();

        let sources_lines = repo.sources_lines();
        assert_eq!(sources_lines.len(), 1);

        // Verify that the server is running
        let url = format!("{}Packages.gz", repo.url().unwrap());
        let response = reqwest::blocking::get(url).unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let mut decoder = GzDecoder::new(response);
        let mut data = String::new();
        use std::io::Read;
        decoder.read_to_string(&mut data).unwrap();
        assert_eq!(data, "Hello, world!");

        // Stop the server
        repo.stop();
    }
}
