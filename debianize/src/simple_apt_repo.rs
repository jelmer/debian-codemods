use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::thread::{self, JoinHandle};
use std::net::SocketAddr;
use std::process::Command;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use flate2::write::GzEncoder;
use flate2::Compression;

pub struct SimpleTrustedAptRepo {
    directory: std::path::PathBuf,
    server_addr: Option<SocketAddr>,
    thread: Option<JoinHandle<()>>,
}

impl SimpleTrustedAptRepo {
    pub fn new(directory: std::path::PathBuf) -> Self {
        SimpleTrustedAptRepo {
            directory,
            server_addr: None,
            thread: None,
        }
    }

    /// Returns the sources.list lines for this repository
    pub fn sources_lines(&self) -> Vec<String> {
        let packages_path = Path::new(&self.directory).join("Packages.gz");
        if packages_path.exists() {
            if let Some(addr) =  self.server_addr {
                vec![format!("deb [trusted=yes] http://{}:{}/ ./", addr.ip(), addr.port())]
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    }

    pub fn start(&mut self) -> io::Result<()> {
        if self.thread.is_some() {
            return Err(io::Error::new(io::ErrorKind::Other, "thread already active"));
        }

        let directory = self.directory.clone();
        let make_svc = make_service_fn(move |_conn| {
            let directory = directory.clone();
            async move {
                Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| {
                    let directory = directory.clone();
                    async move {
                        let path = directory.join(req.uri().path());
                        match fs::read(path) {
                            Ok(contents) => Ok::<_, hyper::Error>(Response::new(Body::from(contents))),
                            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Response::builder()
                                .status(404)
                                .body(Body::from("File not found"))
                                .unwrap()),
                            Err(e) => {
                                log::error!("Failed to read file: {}", e);
                                Ok(Response::builder()
                                    .status(500)
                                    .body(Body::from("Internal server error"))
                                    .unwrap())
                            }
                        }
                    }
                }))
            }
        });

        let addr = SocketAddr::from(([127, 0, 0, 1], 0));
        let server = Server::bind(&addr).serve(make_svc);

        let server_addr = server.local_addr();
        self.server_addr = Some(server_addr);

        let server_future = server.with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        });

        let handle = thread::spawn(move || {
            tokio::runtime::Runtime::new().unwrap().block_on(server_future).unwrap();
        });

        log::info!("Local apt repo started at http://{}:{}/", server_addr.ip(), server_addr.port());
        self.thread = Some(handle);

        Ok(())
    }

    /// Stop the server
    pub fn stop(&mut self) {
        if let Some(thread) = self.thread.take() {
            // Here we rely on the hyper server to shut down when the thread finishes.
            thread.join().unwrap();
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
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to run dpkg-scanpackages"));
        }

        Ok(())
    }
}

// Implementing the Drop trait to mimic the __exit__ method
impl Drop for SimpleTrustedAptRepo {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_simple() {
        let td = tempfile::tempdir().unwrap();
        let mut repo = SimpleTrustedAptRepo::new(td.path().to_path_buf());

        let sources_lines = repo.sources_lines();
        assert_eq!(sources_lines.len(), 0);

        // Start the server
        repo.start().unwrap();

        let sources_lines = repo.sources_lines();
        assert_eq!(sources_lines.len(), 1);

        // Perform some operations...

        // Stop the server
        repo.stop();
    }
}
