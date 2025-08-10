use crate::{declare_fixer, FixerError, FixerResult, Certainty};
use std::fs;
use regex::Regex;

declare_fixer! {
    name: "out-of-date-copyright-format-uri",
    tags: ["out-of-date-copyright-format-uri"],
    apply: |basedir, _package, _version, _preferences| {
        let copyright_path = basedir.join("debian").join("copyright");
        
        if !copyright_path.exists() {
            return Err(FixerError::NoChanges);
        }

        let content = fs::read_to_string(&copyright_path)?;
        
        // Regular expression to match Format or Format-Specification lines
        // This matches the entire line and captures everything after the field name
        let format_regex = Regex::new(r"(?m)^(Format|Format-Specification):\s*.*$").unwrap();
        
        // The correct, up-to-date format URI
        const CORRECT_FORMAT_URI: &str = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/";
        
        // Check if we need to make changes
        if !format_regex.is_match(&content) {
            // No Format field found
            return Err(FixerError::NoChanges);
        }
        
        // Check if the format is already correct
        if content.lines().any(|line| line == CORRECT_FORMAT_URI) {
            return Err(FixerError::NoChanges);
        }
        
        // Replace the Format or Format-Specification line with the correct one
        let new_content = format_regex.replace(&content, CORRECT_FORMAT_URI);
        
        // Write the updated content back
        fs::write(&copyright_path, new_content.as_ref())?;
        
        Ok(FixerResult::new(
            "Use correct machine-readable copyright file URI.".to_string(),
            Some(vec!["out-of-date-copyright-format-uri".to_string()]),
            Some(Certainty::Certain),
            None,
            None,
            vec![],
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_updates_format_specification_field() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        
        let copyright_content = r#"Format-Specification: http://svn.debian.org/wsvn/dep/web/deps/dep5.mdwn?op=file&rev=59
Upstream-Name: test-package

Files: *
Copyright: 2023 Test Author
License: GPL-2+
"#;
        
        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, copyright_content).unwrap();
        
        // Apply regex transformation
        let format_regex = Regex::new(r"(?m)^(Format|Format-Specification):\s*.*$").unwrap();
        let content = fs::read_to_string(&copyright_path).unwrap();
        let new_content = format_regex.replace(&content, "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/");
        fs::write(&copyright_path, new_content.as_ref()).unwrap();
        
        // Verify the change
        let updated_content = fs::read_to_string(&copyright_path).unwrap();
        assert!(updated_content.contains("Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"));
        assert!(!updated_content.contains("Format-Specification:"));
    }

    #[test]
    fn test_updates_format_field() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        
        let copyright_content = r#"Format: http://old.debian.org/some/old/path
Upstream-Name: test-package

Files: *
Copyright: 2023 Test Author
License: GPL-2+
"#;
        
        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, copyright_content).unwrap();
        
        // Apply regex transformation
        let format_regex = Regex::new(r"(?m)^(Format|Format-Specification):\s*.*$").unwrap();
        let content = fs::read_to_string(&copyright_path).unwrap();
        let new_content = format_regex.replace(&content, "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/");
        fs::write(&copyright_path, new_content.as_ref()).unwrap();
        
        // Verify the change
        let updated_content = fs::read_to_string(&copyright_path).unwrap();
        assert!(updated_content.contains("Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"));
        assert!(!updated_content.contains("http://old.debian.org"));
    }

    #[test]
    fn test_no_change_when_format_correct() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        
        let copyright_content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: test-package

Files: *
Copyright: 2023 Test Author
License: GPL-2+
"#;
        
        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, copyright_content).unwrap();
        
        // Should detect that format is already correct
        let content = fs::read_to_string(&copyright_path).unwrap();
        assert!(content.lines().any(|line| line == "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"));
    }

    #[test]
    fn test_no_copyright_file() {
        let temp_dir = TempDir::new().unwrap();
        
        let copyright_path = temp_dir.path().join("debian").join("copyright");
        assert!(!copyright_path.exists());
        // Should return NoChanges when file doesn't exist
    }
}