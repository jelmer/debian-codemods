use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let tests_control_path = base_path.join("debian/tests/control");

    if !tests_control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Read the file and check if it's empty or contains only whitespace
    let content = fs::read_to_string(&tests_control_path)?;
    if !content.trim().is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Remove the empty control file
    fs::remove_file(&tests_control_path)?;

    // Check if the tests directory is now empty and remove it if so
    let tests_dir = base_path.join("debian/tests");
    if tests_dir.exists() {
        match fs::read_dir(&tests_dir) {
            Ok(mut entries) => {
                if entries.next().is_none() {
                    // Directory is empty, remove it
                    fs::remove_dir(&tests_dir)?;
                }
            }
            Err(_) => {
                // If we can't read the directory, just leave it
            }
        }
    }

    Ok(FixerResult::builder("Remove empty debian/tests/control.")
        .fixed_tags(vec!["empty-debian-tests-control"])
        .certainty(crate::Certainty::Certain)
        .build())
}

declare_fixer! {
    name: "empty-debian-tests-control",
    tags: ["empty-debian-tests-control"],
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
    fn test_remove_empty_tests_control() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let tests_control_path = tests_dir.join("control");
        fs::write(&tests_control_path, "").unwrap(); // Empty file

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove empty debian/tests/control.");
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Both the file and directory should be removed
        assert!(!tests_control_path.exists());
        assert!(!tests_dir.exists());
    }

    #[test]
    fn test_remove_whitespace_only_tests_control() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let tests_control_path = tests_dir.join("control");
        fs::write(&tests_control_path, "   \n\t  \n  ").unwrap(); // Only whitespace

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Both the file and directory should be removed
        assert!(!tests_control_path.exists());
        assert!(!tests_dir.exists());
    }

    #[test]
    fn test_keep_non_empty_tests_control() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let tests_control_path = tests_dir.join("control");
        fs::write(&tests_control_path, "Tests: autopkgtest\nDepends: @").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // File should still exist
        assert!(tests_control_path.exists());
        assert!(tests_dir.exists());
    }

    #[test]
    fn test_keep_tests_dir_with_other_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let tests_control_path = tests_dir.join("control");
        let other_file_path = tests_dir.join("other-test");
        fs::write(&tests_control_path, "").unwrap(); // Empty file
        fs::write(&other_file_path, "some content").unwrap(); // Other file

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Control file should be removed but directory should remain
        assert!(!tests_control_path.exists());
        assert!(tests_dir.exists());
        assert!(other_file_path.exists());
    }

    #[test]
    fn test_no_tests_control_file() {
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
