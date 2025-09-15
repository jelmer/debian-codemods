use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");

    // Check if debian/rules exists
    let metadata = match fs::metadata(&rules_path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File doesn't exist, nothing to fix
            return Err(FixerError::NoChanges);
        }
        Err(e) => {
            return Err(FixerError::Other(format!(
                "Failed to stat debian/rules: {}",
                e
            )))
        }
    };

    // Check if the file is already executable (any of the execute bits set)
    let mode = metadata.permissions().mode();
    if (mode & 0o111) != 0 {
        // Already executable, nothing to fix
        return Err(FixerError::NoChanges);
    }

    // Make the file executable (755)
    let mut perms = metadata.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&rules_path, perms)?;

    Ok(FixerResult::builder("Make debian/rules executable.")
        .fixed_tags(vec!["debian-rules-not-executable"])
        .certainty(crate::Certainty::Certain)
        .build())
}

declare_fixer! {
    name: "debian-rules-not-executable",
    tags: ["debian-rules-not-executable"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn test_make_rules_executable() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, "#!/usr/bin/make -f\n%:\n\tdh $@\n").unwrap();

        // Make it non-executable (644)
        let mut perms = fs::metadata(&rules_path).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&rules_path, perms).unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Make debian/rules executable.");
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that the file is now executable
        let mode = fs::metadata(&rules_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[test]
    fn test_rules_already_executable() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, "#!/usr/bin/make -f\n%:\n\tdh $@\n").unwrap();

        // Make it executable (755)
        let mut perms = fs::metadata(&rules_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&rules_path, perms).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
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
