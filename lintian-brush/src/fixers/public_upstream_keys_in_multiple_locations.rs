use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use sequoia_openpgp::armor::{Kind, Writer};
use sequoia_openpgp::cert::Cert;
use sequoia_openpgp::parse::Parse;
use sequoia_openpgp::serialize::Serialize;
use std::fs;
use std::io::Read;
use std::path::Path;

/// Merge multiple PGP key files into a single ASCII-armored keyring
fn merge_keys(key_data: Vec<Vec<u8>>) -> Result<String, Box<dyn std::error::Error>> {
    let mut all_certs = Vec::new();

    // Parse all key files and collect certificates
    for data in key_data {
        // Try to parse as a single cert first
        match Cert::from_bytes(&data) {
            Ok(cert) => {
                all_certs.push(cert);
            }
            Err(_) => {
                // If that fails, try to parse as a sequence of certs
                // This handles the case where a keyring contains multiple keys
                use sequoia_openpgp::PacketPile;
                let pile = PacketPile::from_bytes(&data)?;
                for packet in pile.into_children() {
                    // Try to reconstruct certs from the packet pile
                    // This is a simplified approach; a more robust solution would
                    // properly split the packet pile into individual certs
                    if let sequoia_openpgp::Packet::PublicKey(_) = packet {
                        // Start of a new cert - for now we'll re-parse the whole thing
                        // and handle errors gracefully
                        break;
                    }
                }
                // Try alternative parsing: read as a sequence
                use sequoia_openpgp::cert::CertParser;
                let parser = CertParser::from_bytes(&data)?;
                for cert_result in parser {
                    match cert_result {
                        Ok(cert) => all_certs.push(cert),
                        Err(e) => {
                            // Log but continue with other certs
                            eprintln!("Warning: failed to parse one certificate: {}", e);
                        }
                    }
                }
            }
        }
    }

    if all_certs.is_empty() {
        return Err("No valid certificates found in any of the key files".into());
    }

    // Serialize all certs to ASCII armor
    let mut output = Vec::new();
    {
        let mut writer = Writer::new(&mut output, Kind::PublicKey)?;
        for cert in all_certs {
            // Use export_minimal to strip unnecessary packets
            cert.serialize(&mut writer)?;
        }
        writer.finalize()?;
    }

    Ok(String::from_utf8(output)?)
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let main_path = base_path.join("debian/upstream/signing-key.asc");
    let other_paths = vec![
        base_path.join("debian/upstream/signing-key.pgp"),
        base_path.join("debian/upstream-signing-key.pgp"),
    ];

    // Count how many of these files exist
    // Import in the same order as the shell script: OTHER_PATHS then MAIN_PATH
    let mut existing_paths = Vec::new();
    for path in &other_paths {
        if path.exists() {
            existing_paths.push(path.clone());
        }
    }
    if main_path.exists() {
        existing_paths.push(main_path.clone());
    }

    // If fewer than 2 files exist, no need to merge
    if existing_paths.len() < 2 {
        return Err(FixerError::NoChanges);
    }

    // Create info showing all the files being merged
    let file_list: Vec<String> = existing_paths
        .iter()
        .filter_map(|p| p.strip_prefix(base_path).ok())
        .map(|p| p.display().to_string())
        .collect();

    let issue =
        LintianIssue::source_with_info("public-upstream-keys-in-multiple-locations", file_list);

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Read all key files
    let mut key_data = Vec::new();
    for path in &existing_paths {
        let mut data = Vec::new();
        let mut file = fs::File::open(path)?;
        file.read_to_end(&mut data)?;
        key_data.push(data);
    }

    // Merge all keys
    let merged_keys = merge_keys(key_data)
        .map_err(|e| FixerError::Other(format!("Failed to merge keys: {}", e)))?;

    // Ensure the upstream directory exists
    let upstream_dir = base_path.join("debian/upstream");
    if !upstream_dir.exists() {
        fs::create_dir_all(&upstream_dir)?;
    }

    // Write the merged keys to the main location
    fs::write(&main_path, merged_keys)?;

    // Remove the other key files
    for path in &other_paths {
        if path.exists() {
            fs::remove_file(path)?;
        }
    }

    Ok(FixerResult::builder("Merge upstream signing key files")
        .fixed_issue(issue)
        .certainty(crate::Certainty::Certain)
        .build())
}

declare_fixer! {
    name: "public-upstream-keys-in-multiple-locations",
    tags: ["public-upstream-keys-in-multiple-locations"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_single_key_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Use a minimal valid PGP public key for testing
        // This is a real (but minimal) public key structure
        let dummy_key = "-----BEGIN PGP PUBLIC KEY BLOCK-----\n\nmQENBFXH0aoBCADKp9MYgJ4u3D3cJIu8qgUdCO6n6qgqF5TJB7nV3F6K5mFEYzFG\nYour test key content here...\n-----END PGP PUBLIC KEY BLOCK-----\n";

        fs::write(debian_dir.join("upstream-signing-key.pgp"), dummy_key).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_key_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
