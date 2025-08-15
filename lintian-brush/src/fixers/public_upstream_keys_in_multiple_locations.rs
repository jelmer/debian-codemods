use crate::{declare_fixer, FixerError, FixerResult};
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

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
    
    // Get GPG command from environment or use default
    let gpg_env = env::var("GPG").unwrap_or_else(|_| "gpg".to_string());
    let gpg_parts: Vec<&str> = gpg_env.split_whitespace().collect();
    let gpg_cmd = gpg_parts[0];
    let gpg_args = &gpg_parts[1..];
    
    // Check if gpg is available
    let mut gpg_check = Command::new(gpg_cmd);
    for arg in gpg_args {
        gpg_check.arg(arg);
    }
    let output = gpg_check.arg("--version").output();
    
    if output.is_err() || !output.unwrap().status.success() {
        return Err(FixerError::Other(
            "gpg is not available".to_string()
        ));
    }
    
    // Create temporary directory and keyring
    let temp_dir = TempDir::new()?;
    let temp_keyring = temp_dir.path().join("keyring.gpg");
    
    // Import all existing keys into the temporary keyring
    for path in &existing_paths {
        let mut cmd = Command::new(gpg_cmd);
        for arg in gpg_args {
            cmd.arg(arg);
        }
        let output = cmd
            .arg("--quiet")
            .arg("--no-default-keyring")
            .arg("--keyring")
            .arg(&temp_keyring)
            .arg("--import")
            .arg(path)
            .env("GNUPGHOME", temp_dir.path())
            .output()?;
        
        if !output.status.success() {
            return Err(FixerError::Other(format!(
                "Failed to import key from {:?}: {}",
                path,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
    }
    
    // Export all keys to the main location in ASCII armor format
    let mut cmd = Command::new(gpg_cmd);
    for arg in gpg_args {
        cmd.arg(arg);
    }
    let output = cmd
        .arg("--quiet")
        .arg("--no-default-keyring")
        .arg("--keyring")
        .arg(&temp_keyring)
        .arg("--export-options")
        .arg("export-minimal")
        .arg("--export")
        .arg("--armor")
        .env("GNUPGHOME", temp_dir.path())
        .output()?;
    
    if !output.status.success() {
        return Err(FixerError::Other(format!(
            "Failed to export merged keys: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    
    // Ensure the upstream directory exists
    let upstream_dir = base_path.join("debian/upstream");
    if !upstream_dir.exists() {
        fs::create_dir_all(&upstream_dir)?;
    }
    
    // Write the merged keys to the main location
    fs::write(&main_path, output.stdout)?;
    
    // Remove the other key files
    for path in &other_paths {
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    
    Ok(FixerResult::builder("Merge upstream signing key files.")
        .fixed_tags(vec!["public-upstream-keys-in-multiple-locations"])
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
        // Check if gpg is available for tests
        let gpg_check = Command::new("gpg")
            .arg("--version")
            .output();
        
        if gpg_check.is_err() || !gpg_check.unwrap().status.success() {
            eprintln!("Skipping test: gpg not available");
            return;
        }
        
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();
        
        // Create a dummy key file (just needs to be valid enough for gpg to import)
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