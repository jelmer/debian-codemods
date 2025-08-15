use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    // Check if we're in opinionated mode
    let opinionated = std::env::var("OPINIONATED")
        .map(|v| v.to_lowercase() != "no" && v != "0" && v != "false")
        .unwrap_or(false);
    
    if !opinionated {
        return Err(FixerError::NoChanges);
    }
    
    let series_path = base_path.join("debian/patches/series");
    
    // Check if the file exists
    if !series_path.exists() {
        return Err(FixerError::NoChanges);
    }
    
    // Read the file and check if it's empty or contains only whitespace
    let content = fs::read_to_string(&series_path)?;
    if !content.trim().is_empty() {
        // File has content, don't remove it
        return Err(FixerError::NoChanges);
    }
    
    // Remove the empty file
    fs::remove_file(&series_path)?;
    
    Ok(FixerResult::builder("Remove empty debian/patches/series.")
        .certainty(crate::Certainty::Certain)
        .build())
}

declare_fixer! {
    name: "empty-debian-patches-series",
    tags: [],
    apply: |basedir, _package, _version, preferences| {
        // Set OPINIONATED environment variable from preferences
        if let Some(opinionated) = preferences.opinionated {
            if opinionated {
                std::env::set_var("OPINIONATED", "yes");
            } else {
                std::env::set_var("OPINIONATED", "no");
            }
        }
        
        let result = run(basedir);
        
        // Clean up environment variable
        std::env::remove_var("OPINIONATED");
        
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    
    #[test]
    fn test_remove_empty_series_opinionated() {
        std::env::set_var("OPINIONATED", "yes");
        
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let patches_dir = base_path.join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        
        let series_path = patches_dir.join("series");
        fs::write(&series_path, "").unwrap(); // Empty file
        
        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove empty debian/patches/series.");
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));
        
        // Check that the file was removed
        assert!(!series_path.exists());
        
        std::env::remove_var("OPINIONATED");
    }
    
    #[test]
    fn test_remove_whitespace_only_series_opinionated() {
        std::env::set_var("OPINIONATED", "yes");
        
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let patches_dir = base_path.join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        
        let series_path = patches_dir.join("series");
        fs::write(&series_path, "   \n\t  \n  ").unwrap(); // Only whitespace
        
        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));
        
        // Check that the file was removed
        assert!(!series_path.exists());
        
        std::env::remove_var("OPINIONATED");
    }
    
    #[test]
    fn test_not_opinionated() {
        std::env::set_var("OPINIONATED", "no");
        
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let patches_dir = base_path.join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        
        let series_path = patches_dir.join("series");
        fs::write(&series_path, "").unwrap(); // Empty file
        
        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
        
        // File should still exist
        assert!(series_path.exists());
        
        std::env::remove_var("OPINIONATED");
    }
    
    #[test]
    fn test_keep_non_empty_series() {
        std::env::set_var("OPINIONATED", "yes");
        
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let patches_dir = base_path.join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        
        let series_path = patches_dir.join("series");
        fs::write(&series_path, "some-patch.patch\n").unwrap();
        
        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
        
        // File should still exist
        assert!(series_path.exists());
        
        std::env::remove_var("OPINIONATED");
    }
    
    #[test]
    fn test_no_series_file() {
        std::env::set_var("OPINIONATED", "yes");
        
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let patches_dir = base_path.join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        
        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
        
        std::env::remove_var("OPINIONATED");
    }
    
    #[test]
    fn test_no_debian_dir() {
        std::env::set_var("OPINIONATED", "yes");
        
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        
        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
        
        std::env::remove_var("OPINIONATED");
    }
}