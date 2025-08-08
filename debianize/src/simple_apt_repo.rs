use flate2::write::GzEncoder;
use flate2::Compression;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
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

    pub fn directory(&self) -> &Path {
        &self.directory
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

        // Create an async function that will handle requests
        async fn handle_request(
            req: Request<hyper::body::Incoming>,
            directory: Arc<PathBuf>,
        ) -> Result<Response<Full<Bytes>>, hyper::Error> {
            let path = directory.join(req.uri().path().trim_start_matches('/'));
            match fs::read(path) {
                Ok(contents) => Ok(Response::new(Full::new(Bytes::from(contents)))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Full::new(Bytes::from("File not found")))
                    .unwrap()),
                Err(e) => {
                    log::error!("Error reading file: {}", e);
                    Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Full::new(Bytes::from("Internal server error")))
                        .unwrap())
                }
            }
        }

        let addr = SocketAddr::from(([127, 0, 0, 1], 0));

        let handle = thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let listener = match tokio::net::TcpListener::bind(addr).await {
                    Ok(l) => l,
                    Err(e) => {
                        log::error!("Failed to bind to address: {}", e);
                        return;
                    }
                };

                let bound_addr = listener.local_addr().unwrap();
                *server_addr.lock().unwrap() = Some(bound_addr);
                start_tx.send(bound_addr).unwrap();

                let directory_clone = Arc::clone(&directory);
                let (close_tx, mut close_rx) = tokio::sync::mpsc::channel::<()>(1);

                // Spawn a task to handle the shutdown signal
                tokio::spawn(async move {
                    shutdown_rx.await.ok();
                    let _ = close_tx.send(()).await;
                });

                // Accept connections in a loop
                loop {
                    tokio::select! {
                        // Check if we should shut down
                        _ = close_rx.recv() => {
                            break;
                        }
                        // Accept new connections
                        conn_result = listener.accept() => {
                            match conn_result {
                                Ok((stream, _)) => {
                                    let io = TokioIo::new(stream);
                                    let directory_ref = Arc::clone(&directory_clone);

                                    // Spawn a task to handle the connection
                                    tokio::task::spawn(async move {
                                        let service = service_fn(move |req| {
                                            let dir_ref = Arc::clone(&directory_ref);
                                            handle_request(req, dir_ref)
                                        });

                                        if let Err(err) = http1::Builder::new()
                                            .serve_connection(io, service)
                                            .await {
                                            log::error!("Failed to serve connection: {}", err);
                                        }
                                    });
                                }
                                Err(e) => {
                                    log::error!("Failed to accept connection: {}", e);
                                }
                            }
                        }
                    }
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

    /// Add a .deb package to the repository
    ///
    /// This copies the specified .deb file to the repository directory
    /// and refreshes the metadata.
    pub fn add_package(&self, deb_path: &Path) -> io::Result<()> {
        if !deb_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Package file not found: {:?}", deb_path),
            ));
        }

        let filename = deb_path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid deb filename"))?;

        let dest_path = self.directory.join(filename);
        fs::copy(deb_path, &dest_path)?;

        log::info!("Added package {:?} to repository", filename);

        // Refresh the repository metadata
        self.refresh()?;

        Ok(())
    }

    /// Add multiple .deb packages to the repository
    pub fn add_packages(&self, deb_paths: &[&Path]) -> io::Result<()> {
        for path in deb_paths {
            let filename = path.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "Invalid deb filename")
            })?;

            let dest_path = self.directory.join(filename);
            fs::copy(path, &dest_path)?;

            log::info!("Added package {:?} to repository", filename);
        }

        // Refresh the repository metadata once after adding all packages
        self.refresh()?;

        Ok(())
    }

    /// List all .deb packages in the repository
    pub fn list_packages(&self) -> io::Result<Vec<String>> {
        let mut packages = Vec::new();

        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(extension) = path.extension() {
                if extension == "deb" {
                    if let Some(filename) = path.file_name() {
                        packages.push(filename.to_string_lossy().to_string());
                    }
                }
            }
        }

        Ok(packages)
    }

    /// Remove a package from the repository
    pub fn remove_package(&self, package_name: &str) -> io::Result<()> {
        let package_path = self.directory.join(package_name);

        if !package_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Package not found: {}", package_name),
            ));
        }

        fs::remove_file(&package_path)?;
        log::info!("Removed package {} from repository", package_name);

        // Refresh the repository metadata
        self.refresh()?;

        Ok(())
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
            log::error!(
                "dpkg-scanpackages failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
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
    use std::fs::File;
    use std::io::Read;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

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
        // Verify sources line format includes "trusted=yes" option
        assert!(sources_lines[0].contains("[trusted=yes]"));
        assert!(sources_lines[0].starts_with("deb [trusted=yes] http://127.0.0.1:"));
        assert!(sources_lines[0].ends_with("/ ./"));

        // Verify that the server is running
        let url = format!("{}Packages.gz", repo.url().unwrap());
        let response = reqwest::blocking::get(url).unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let mut decoder = GzDecoder::new(response);
        let mut data = String::new();
        decoder.read_to_string(&mut data).unwrap();
        assert_eq!(data, "Hello, world!");

        // Stop the server
        repo.stop();
    }

    #[test]
    fn test_directory() {
        let td = tempfile::tempdir().unwrap();
        let repo = SimpleTrustedAptRepo::new(td.path().to_path_buf());

        assert_eq!(repo.directory(), td.path());
    }

    #[test]
    fn test_url_when_not_started() {
        let td = tempfile::tempdir().unwrap();
        let repo = SimpleTrustedAptRepo::new(td.path().to_path_buf());

        assert_eq!(repo.url(), None);
    }

    #[test]
    fn test_url_when_started() {
        let td = tempfile::tempdir().unwrap();
        let mut repo = SimpleTrustedAptRepo::new(td.path().to_path_buf());

        repo.start().unwrap();

        let url = repo.url().unwrap();
        assert!(url.to_string().starts_with("http://127.0.0.1:"));
        assert!(url.to_string().ends_with("/"));

        repo.stop();
    }

    #[test]
    fn test_start_twice_fails() {
        let td = tempfile::tempdir().unwrap();
        let mut repo = SimpleTrustedAptRepo::new(td.path().to_path_buf());

        repo.start().unwrap();
        let err = repo.start().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Other);
        assert_eq!(err.to_string(), "server already active");

        repo.stop();
    }

    #[test]
    fn test_stop_when_not_started() {
        let td = tempfile::tempdir().unwrap();
        let mut repo = SimpleTrustedAptRepo::new(td.path().to_path_buf());

        // Should not panic
        repo.stop();
    }

    #[test]
    fn test_server_404() {
        let td = tempfile::tempdir().unwrap();
        let mut repo = SimpleTrustedAptRepo::new(td.path().to_path_buf());

        repo.start().unwrap();

        // Request a file that doesn't exist
        let url = format!("{}nonexistent-file", repo.url().unwrap());
        let response = reqwest::blocking::get(url).unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);

        repo.stop();
    }

    #[test]
    fn test_server_500() {
        let td = tempfile::tempdir().unwrap();
        let mut repo = SimpleTrustedAptRepo::new(td.path().to_path_buf());

        repo.start().unwrap();

        // Create a directory that can't be read as a file
        let dir_path = td.path().join("directory");
        fs::create_dir(&dir_path).unwrap();

        // Request the directory as a file - this should trigger a server error
        let url = format!("{}directory", repo.url().unwrap());
        let response = reqwest::blocking::get(url).unwrap();
        assert_eq!(
            response.status(),
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        );

        repo.stop();
    }

    #[test]
    fn test_refresh() {
        // Skip this test if dpkg-scanpackages is not available
        if Command::new("dpkg-scanpackages")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }

        let td = tempfile::tempdir().unwrap();
        let repo = SimpleTrustedAptRepo::new(td.path().to_path_buf());

        // Refresh should create a Packages.gz file
        repo.refresh().unwrap();

        // Verify that Packages.gz was created
        let packages_path = td.path().join("Packages.gz");
        assert!(packages_path.exists());

        // Verify the content is a valid gzip file with dpkg-scanpackages output
        let file = File::open(packages_path).unwrap();
        let mut decoder = GzDecoder::new(file);
        let mut content = String::new();
        decoder.read_to_string(&mut content).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn test_refresh_failed_command() {
        // Only run this test if dpkg-scanpackages is available
        if Command::new("dpkg-scanpackages")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }

        let td = tempfile::tempdir().unwrap();
        let repo = SimpleTrustedAptRepo::new(td.path().to_path_buf());

        // Create a dummy .deb file
        let deb_path = td.path().join("test_1.0-1_all.deb");
        File::create(&deb_path).unwrap();

        // Make the directory read-only to force a failure
        let mut perms = fs::metadata(td.path()).unwrap().permissions();
        perms.set_mode(0o500); // r-x for owner, nothing for others
        fs::set_permissions(td.path(), perms).unwrap();

        // Refresh should fail because we can't write to the directory
        let result = repo.refresh();
        assert!(result.is_err());

        // Reset permissions for cleanup
        let mut perms = fs::metadata(td.path()).unwrap().permissions();
        perms.set_mode(0o755); // rwx for owner, rx for others
        fs::set_permissions(td.path(), perms).unwrap();
    }

    #[test]
    fn test_drop_stops_server() {
        let td = tempfile::tempdir().unwrap();
        let url;

        {
            let mut repo = SimpleTrustedAptRepo::new(td.path().to_path_buf());
            repo.start().unwrap();
            url = repo.url().unwrap().to_string();

            // Server should be running
            let response = reqwest::blocking::get(format!("{}Packages.gz", url));
            assert!(response.is_ok());
            assert_eq!(response.unwrap().status(), reqwest::StatusCode::NOT_FOUND);

            // Let repo drop out of scope, which should stop the server
        }

        // Server should no longer be running
        let response = reqwest::blocking::get(url);
        assert!(response.is_err());
    }
}
