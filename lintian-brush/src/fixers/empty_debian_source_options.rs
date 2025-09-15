use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let options_path = base_path.join("debian/source/options");

    // Check if the file exists
    if !options_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Read the file and check if it's empty or contains only whitespace
    let content = fs::read_to_string(&options_path)?;
    if !content.trim().is_empty() {
        // File has content, don't remove it
        return Err(FixerError::NoChanges);
    }

    // Remove the empty file
    fs::remove_file(&options_path)?;

    Ok(FixerResult::builder("Remove empty debian/source/options.")
        .certainty(crate::Certainty::Certain)
        .build())
}

declare_fixer! {
    name: "empty-debian-source-options",
    tags: [],
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
    fn test_remove_empty_options() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let source_dir = base_path.join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();

        let options_path = source_dir.join("options");
        fs::write(&options_path, "").unwrap(); // Empty file

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove empty debian/source/options.");
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that the file was removed
        assert!(!options_path.exists());
    }

    #[test]
    fn test_remove_whitespace_only_options() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let source_dir = base_path.join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();

        let options_path = source_dir.join("options");
        fs::write(&options_path, "   \n\t  \n  ").unwrap(); // Only whitespace

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that the file was removed
        assert!(!options_path.exists());
    }

    #[test]
    fn test_keep_non_empty_options() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let source_dir = base_path.join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();

        let options_path = source_dir.join("options");
        fs::write(&options_path, "compression = xz\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // File should still exist
        assert!(options_path.exists());
    }

    #[test]
    fn test_no_options_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let source_dir = base_path.join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();

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
