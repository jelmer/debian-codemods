use crate::{FixerError, FixerResult};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");

    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let metadata = fs::metadata(&rules_path)?;
    let current_mode = metadata.permissions().mode();

    // Check if it's already executable (has execute bit for owner, group, or others)
    if current_mode & 0o111 != 0 {
        return Err(FixerError::NoChanges);
    }

    // Set permissions to 0o755 (rwxr-xr-x)
    let mut perms = metadata.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&rules_path, perms)?;

    Ok(FixerResult::builder("Mark debian/rules as executable.")
        .certainty(crate::Certainty::Certain)
        .build())
}

declare_fixer! {
    name: "rules-not-executable",
    tags: [],
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

        // Set non-executable permissions (644)
        let mut perms = fs::metadata(&rules_path).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&rules_path, perms).unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Mark debian/rules as executable.");
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that file is now executable
        let metadata = fs::metadata(&rules_path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o755);
    }

    #[test]
    fn test_already_executable() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, "#!/usr/bin/make -f\n%:\n\tdh $@\n").unwrap();

        // Set executable permissions (755)
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
