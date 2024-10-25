//! Interacting with snapshot.debian.org
use debversion::Version;
use sha1::Digest;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
/// A struct representing a file in a snapshot
struct FileInfo {
    archive_name: String,

    /// The date the file was first seen
    first_seen: chrono::DateTime<chrono::Utc>,

    /// The name of the file
    name: String,
    /// Path to the file
    path: String,

    /// The size of the file
    size: usize,
}

#[derive(Debug)]
/// An error that can occur while downloading a snapshot
pub enum Error {
    /// An error occurred while downloading a snapshot
    SnapshotDownloadError(String, reqwest::Error, Option<bool>),

    /// The snapshot is missing
    SnapshotMissing(String, Version),

    /// The hash of a file in the snapshot does not match the expected hash
    SnapshotHashMismatch {
        /// The filename of the file
        filename: String,

        /// The actual hash of the file
        actual_hash: String,

        /// The expected hash of the file
        expected_hash: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::SnapshotDownloadError(url, e, Some(true)) => {
                write!(f, "Snapshot download error: {} (server error) {}", url, e)
            }
            Error::SnapshotDownloadError(url, e, _) => {
                write!(f, "Snapshot download error: {} {}", url, e)
            }
            Error::SnapshotMissing(package, version) => {
                write!(f, "Snapshot missing: {} {}", package, version)
            }
            Error::SnapshotHashMismatch {
                filename,
                actual_hash,
                expected_hash,
            } => {
                write!(
                    f,
                    "Hash mismatch for {}: expected {} but got {}",
                    filename, expected_hash, actual_hash
                )
            }
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct FileHash {
    hash: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct SrcFiles {
    fileinfo: HashMap<String, Vec<FileInfo>>,

    package: String,

    version: Version,

    result: Vec<FileHash>,

    #[serde(rename = "_comment")]
    comment: String,
}

/// Download a snapshot of a package
pub fn download_snapshot(
    package: &str,
    version: &Version,
    output_dir: &Path,
) -> Result<PathBuf, Error> {
    log::info!("Downloading {} {}", package, version);
    let srcfiles_url = format!(
        "https://snapshot.debian.org/mr/package/{}/{}/srcfiles?fileinfo=1",
        package, version
    );
    let response = match reqwest::blocking::get(&srcfiles_url) {
        Ok(response) => response,
        Err(e) => match e.status() {
            Some(reqwest::StatusCode::NOT_FOUND) => {
                return Err(Error::SnapshotMissing(package.to_owned(), version.clone()));
            }
            Some(s) => {
                return Err(Error::SnapshotDownloadError(
                    srcfiles_url,
                    e,
                    if s.is_server_error() {
                        Some(true)
                    } else {
                        None
                    },
                ));
            }
            None => {
                return Err(Error::SnapshotDownloadError(srcfiles_url, e, None));
            }
        },
    };
    let srcfiles = response.json::<SrcFiles>().unwrap();

    let mut files = HashMap::new();

    for (hsh, entries) in srcfiles.fileinfo.iter() {
        for entry in entries {
            files.insert(entry.name.clone(), hsh.clone());
        }
    }

    for (filename, hsh) in files.iter() {
        let local_path = output_dir.join(filename);
        if local_path.exists() {
            let mut f = File::open(&local_path).unwrap();
            let mut actual_hsh = sha1::Sha1::new();
            std::io::copy(&mut f, &mut actual_hsh).unwrap();
            let actual_hsh = hex::encode(actual_hsh.finalize());
            if actual_hsh != *hsh {
                return Err(Error::SnapshotHashMismatch {
                    filename: filename.clone(),
                    actual_hash: actual_hsh,
                    expected_hash: hsh.clone(),
                });
            }
        } else {
            let mut f = File::create(&local_path).unwrap();
            let url = format!("https://snapshot.debian.org/file/{}", hsh);
            log::info!("Downloading {} -> {}", url, filename);
            let mut response = match reqwest::blocking::get(&url) {
                Ok(response) => response,
                Err(e) => match e.status() {
                    Some(s) => {
                        return Err(Error::SnapshotDownloadError(
                            url,
                            e,
                            if s.is_server_error() {
                                Some(true)
                            } else {
                                None
                            },
                        ));
                    }
                    None => {
                        return Err(Error::SnapshotDownloadError(url, e, None));
                    }
                },
            };
            std::io::copy(&mut response, &mut f).unwrap();
        }
    }

    let mut file_version = srcfiles.version;
    file_version.epoch = None;
    let dsc_filename = format!("{}_{}.dsc", srcfiles.package, file_version);
    Ok(output_dir.join(&dsc_filename))
}
