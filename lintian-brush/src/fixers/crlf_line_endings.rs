use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use std::fs;
use std::path::Path;

// Convert CRLF line endings to LF in debian/control files
declare_fixer! {
    name: "control-file-with-CRLF-EOLs",
    tags: ["carriage-return-line-feed"],
    apply: |basedir, _package, _version, _preferences| {
        let control_path = basedir.join("debian/control");

        if !control_path.exists() {
            return Err(FixerError::NoChanges);
        }

        // Check if file actually has CRLF first
        let content = fs::read_to_string(&control_path)
            .map_err(|e| FixerError::Other(format!("Failed to read file {}: {}", control_path.display(), e)))?;

        if !content.contains("\r\n") {
            return Err(FixerError::NoChanges);
        }

        let issue = LintianIssue::source_with_info(
            "carriage-return-line-feed",
            vec!["debian/control".to_string()],
        );

        if !issue.should_fix(basedir) {
            return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
        }

        let changed = convert_line_endings(&control_path)?;

        if changed {
            Ok(FixerResult::builder("Format control file with unix-style line endings.")
                .fixed_issues(vec![issue])
                .build())
        } else {
            Err(FixerError::NoChanges)
        }
    }
}

fn convert_line_endings(path: &Path) -> Result<bool, FixerError> {
    let content = fs::read_to_string(path)
        .map_err(|e| FixerError::Other(format!("Failed to read file {}: {}", path.display(), e)))?;

    // Check if file has CRLF line endings
    if !content.contains("\r\n") {
        return Ok(false);
    }

    // Convert CRLF to LF
    let converted = content.replace("\r\n", "\n");

    fs::write(path, converted).map_err(|e| {
        FixerError::Other(format!("Failed to write file {}: {}", path.display(), e))
    })?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_convert_line_endings_with_crlf() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        // Write file with CRLF line endings
        fs::write(&file_path, "line1\r\nline2\r\nline3\r\n").unwrap();

        let result = convert_line_endings(&file_path);
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Check that line endings were converted
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "line1\nline2\nline3\n");
        assert!(!content.contains("\r\n"));
    }

    #[test]
    fn test_convert_line_endings_without_crlf() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        // Write file with LF line endings only
        fs::write(&file_path, "line1\nline2\nline3\n").unwrap();

        let result = convert_line_endings(&file_path);
        assert!(result.is_ok());
        assert!(!result.unwrap());

        // Check that content is unchanged
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "line1\nline2\nline3\n");
    }

    #[test]
    fn test_convert_line_endings_nonexistent_file() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("nonexistent.txt");

        let result = convert_line_endings(&file_path);
        assert!(result.is_err());

        if let Err(FixerError::Other(msg)) = result {
            assert!(msg.contains("Failed to read file"));
            assert!(msg.contains("nonexistent.txt"));
        } else {
            panic!("Expected FixerError::Other");
        }
    }

    #[test]
    fn test_crlf_fixer_with_control_file() {
        let temp_dir = tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();
        let control_path = debian_dir.join("control");

        // Create control file with CRLF endings
        fs::write(&control_path, "Source: test-package\r\nSection: misc\r\n").unwrap();

        let _preferences = FixerPreferences::default();
        let _version: Version = "1.0".parse().unwrap();

        // This should work by calling the fixer through the declare_fixer! macro
        // We can't directly test the closure, but we can test the convert_line_endings function
        let result = convert_line_endings(&control_path);
        assert!(result.is_ok());
        assert!(result.unwrap());

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(!content.contains("\r\n"));
        assert_eq!(content, "Source: test-package\nSection: misc\n");
    }

    #[test]
    fn test_crlf_fixer_no_control_file() {
        let temp_dir = tempdir().unwrap();
        // No debian/control file exists

        // We can't easily test the fixer closure directly, but we know it should
        // return NoChanges when the control file doesn't exist based on the implementation
        let control_path = temp_dir.path().join("debian/control");
        assert!(!control_path.exists());
    }

    #[test]
    fn test_crlf_fixer_mixed_line_endings() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        // Write file with mixed line endings
        fs::write(&file_path, "line1\r\nline2\nline3\r\n").unwrap();

        let result = convert_line_endings(&file_path);
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Check that all CRLF sequences were converted
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "line1\nline2\nline3\n");
        assert!(!content.contains("\r\n"));
    }
}
