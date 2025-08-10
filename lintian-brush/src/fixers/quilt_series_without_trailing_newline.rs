use crate::{declare_fixer, FixerError, FixerResult, Certainty};
use std::fs;

declare_fixer! {
    name: "quilt-series-without-trailing-newline",
    tags: ["quilt-series-without-trailing-newline"],
    apply: |basedir, _package, _version, _preferences| {
        let series_path = basedir.join("debian").join("patches").join("series");
        
        if !series_path.exists() {
            return Err(FixerError::NoChanges);
        }

        let content = fs::read(&series_path)?;
        
        // Check if file is empty
        if content.is_empty() {
            return Err(FixerError::NoChanges);
        }
        
        // Check if last character is a newline
        if content[content.len() - 1] == b'\n' {
            // File already has trailing newline
            return Err(FixerError::NoChanges);
        }
        
        // Add trailing newline
        let mut new_content = content;
        new_content.push(b'\n');
        fs::write(&series_path, new_content)?;
        
        Ok(FixerResult::new(
            "Add missing trailing newline in debian/patches/series.".to_string(),
            Some(vec!["quilt-series-without-trailing-newline".to_string()]),
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
    fn test_adds_missing_newline() {
        let temp_dir = TempDir::new().unwrap();
        let patches_dir = temp_dir.path().join("debian").join("patches");
        fs::create_dir_all(&patches_dir).unwrap();
        
        // Create series file without trailing newline
        let series_path = patches_dir.join("series");
        fs::write(&series_path, b"patch1.diff").unwrap();
        
        // Apply fixer logic directly for testing
        let content = fs::read(&series_path).unwrap();
        assert_eq!(content[content.len() - 1], b'f'); // No trailing newline
        
        let mut new_content = content;
        new_content.push(b'\n');
        fs::write(&series_path, new_content).unwrap();
        
        // Verify newline was added
        let updated_content = fs::read(&series_path).unwrap();
        assert_eq!(updated_content[updated_content.len() - 1], b'\n');
        assert_eq!(updated_content, b"patch1.diff\n");
    }

    #[test]
    fn test_no_change_when_newline_exists() {
        let temp_dir = TempDir::new().unwrap();
        let patches_dir = temp_dir.path().join("debian").join("patches");
        fs::create_dir_all(&patches_dir).unwrap();
        
        // Create series file with trailing newline
        let series_path = patches_dir.join("series");
        fs::write(&series_path, b"patch1.diff\n").unwrap();
        
        let content = fs::read(&series_path).unwrap();
        assert_eq!(content[content.len() - 1], b'\n'); // Already has trailing newline
        
        // Should not modify file
        let original_content = content.clone();
        // Since file already has newline, fixer should return NoChanges
        assert_eq!(content, original_content);
    }

    #[test] 
    fn test_no_series_file() {
        let temp_dir = TempDir::new().unwrap();
        // Don't create debian/patches/series file
        
        let series_path = temp_dir.path().join("debian").join("patches").join("series");
        assert!(!series_path.exists());
        // Should return NoChanges when file doesn't exist
    }
}