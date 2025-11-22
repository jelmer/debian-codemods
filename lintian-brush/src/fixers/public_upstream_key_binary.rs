use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::io::Read;
use std::path::Path;

/// Convert a binary PGP key to ASCII armored format using sequoia-openpgp
fn convert_key_to_armor(binary_key: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    use sequoia_openpgp::armor::{Kind, Writer};
    use sequoia_openpgp::cert::Cert;
    use sequoia_openpgp::parse::Parse;
    use sequoia_openpgp::serialize::Serialize;

    // Parse the binary key
    let cert = Cert::from_bytes(binary_key)?;

    // Create an armored writer
    let mut armored = Vec::new();
    {
        let mut writer = Writer::new(&mut armored, Kind::PublicKey)?;
        cert.serialize(&mut writer)?;
        writer.finalize()?;
    }

    Ok(String::from_utf8(armored)?)
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let binary_key_path = base_path.join("debian/upstream/signing-key.pgp");
    let ascii_key_path = base_path.join("debian/upstream/signing-key.asc");

    // Check if the binary key file exists
    if !binary_key_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Read the binary key
    let mut binary_key = Vec::new();
    let mut file = fs::File::open(&binary_key_path)?;
    file.read_to_end(&mut binary_key)?;

    // Convert to ASCII armored format
    let armored_key = convert_key_to_armor(&binary_key)
        .map_err(|e| FixerError::Other(format!("Failed to convert key to armor: {}", e)))?;

    // Write the ASCII armored key
    fs::write(&ascii_key_path, armored_key)?;

    // Remove the binary key file
    fs::remove_file(&binary_key_path)?;

    Ok(FixerResult::builder("Enarmor upstream signing key.").build())
}

declare_fixer! {
    name: "public-upstream-key-binary",
    tags: [],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_convert_binary_key_to_armored() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir_all(&upstream_dir).unwrap();

        // Use the actual test fixture key
        let test_fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/public-upstream-key-binary/simple/in/debian/upstream/signing-key.pgp");

        // Skip test if fixture doesn't exist (e.g., during package build)
        if !test_fixture_path.exists() {
            eprintln!(
                "Skipping test: fixture not found at {:?}",
                test_fixture_path
            );
            return;
        }

        let binary_key = fs::read(&test_fixture_path).unwrap();
        let binary_key_path = upstream_dir.join("signing-key.pgp");
        fs::write(&binary_key_path, &binary_key).unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        // Check that the ASCII armored file was created
        let ascii_key_path = upstream_dir.join("signing-key.asc");
        assert!(ascii_key_path.exists());

        // Check that the binary file was removed
        assert!(!binary_key_path.exists());

        // Verify the ASCII armored file contains valid PGP data
        let ascii_content = fs::read_to_string(&ascii_key_path).unwrap();
        assert!(ascii_content.contains("-----BEGIN PGP PUBLIC KEY BLOCK-----"));
        assert!(ascii_content.contains("-----END PGP PUBLIC KEY BLOCK-----"));

        // Verify the key can be parsed back
        use sequoia_openpgp::cert::Cert;
        use sequoia_openpgp::parse::Parse;
        let _cert = Cert::from_bytes(ascii_content.as_bytes()).unwrap();
    }

    #[test]
    fn test_no_binary_key_file() {
        let temp_dir = TempDir::new().unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
