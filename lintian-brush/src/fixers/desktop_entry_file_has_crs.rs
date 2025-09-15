use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let debian_dir = base_path.join("debian");

    if !debian_dir.exists() {
        return Err(FixerError::NoChanges);
    }

    let mut files_modified = false;

    // Find all .desktop files in the debian directory
    let entries = fs::read_dir(&debian_dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Only process .desktop files
        if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
            continue;
        }

        // Read the file as bytes to preserve exact binary content
        let content = fs::read(&path)?;

        // Check if there are any CR characters
        if !content.contains(&b'\r') {
            continue;
        }

        // Remove all CR characters
        let new_content: Vec<u8> = content.into_iter().filter(|&b| b != b'\r').collect();

        // Write back the modified content
        fs::write(&path, new_content)?;
        files_modified = true;
    }

    if !files_modified {
        return Err(FixerError::NoChanges);
    }

    Ok(FixerResult::builder("Remove CRs from desktop files.")
        .fixed_tags(vec!["desktop-entry-file-has-crs"])
        .certainty(crate::Certainty::Certain)
        .build())
}

declare_fixer! {
    name: "desktop-entry-file-has-crs",
    tags: ["desktop-entry-file-has-crs"],
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
    fn test_remove_crs_from_desktop_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let desktop_path = debian_dir.join("test.desktop");
        // Write content with CRLF line endings
        fs::write(
            &desktop_path,
            b"[Desktop Entry]\r\nType=Application\r\nName=Test\r\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove CRs from desktop files.");
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that CRs were removed
        let content = fs::read(&desktop_path).unwrap();
        assert!(!content.contains(&b'\r'));
        assert_eq!(content, b"[Desktop Entry]\nType=Application\nName=Test\n");
    }

    #[test]
    fn test_multiple_desktop_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create multiple desktop files, some with CRs, some without
        let desktop1_path = debian_dir.join("app1.desktop");
        let desktop2_path = debian_dir.join("app2.desktop");
        let non_desktop_path = debian_dir.join("control");

        fs::write(&desktop1_path, b"[Desktop Entry]\r\nType=Application\r\n").unwrap();
        fs::write(&desktop2_path, b"[Desktop Entry]\nType=Service\n").unwrap(); // No CRs
        fs::write(&non_desktop_path, b"Source: test\r\n").unwrap(); // Non-desktop file with CRs

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that CRs were removed from desktop files only
        let content1 = fs::read(&desktop1_path).unwrap();
        assert!(!content1.contains(&b'\r'));

        let content2 = fs::read(&desktop2_path).unwrap();
        assert!(!content2.contains(&b'\r')); // Already didn't have CRs

        let control_content = fs::read(&non_desktop_path).unwrap();
        assert!(control_content.contains(&b'\r')); // Should be unchanged
    }

    #[test]
    fn test_no_desktop_files_with_crs() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let desktop_path = debian_dir.join("test.desktop");
        // Write content with LF line endings only
        fs::write(&desktop_path, b"[Desktop Entry]\nType=Application\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_desktop_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Only non-desktop files
        fs::write(debian_dir.join("control"), b"Source: test\r\n").unwrap();

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
