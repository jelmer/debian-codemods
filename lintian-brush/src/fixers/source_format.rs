use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult};
use debversion::Version;
use std::fs;
use std::path::{Path, PathBuf};

pub fn run(
    base_path: &Path,
    _package_name: &str,
    current_version: &Version,
    _preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    // Check if this is a debcargo package - skip if so
    if is_debcargo_package(base_path)? {
        return Err(FixerError::NoChanges);
    }

    let format_path = base_path.join("debian/source/format");
    let source_dir = base_path.join("debian/source");

    // Read current format if it exists
    let orig_format = if format_path.exists() {
        Some(fs::read_to_string(&format_path)?.trim().to_string())
    } else {
        None
    };

    // Only process if missing or "1.0"
    if let Some(ref fmt) = orig_format {
        if fmt != "1.0" {
            return Err(FixerError::NoChanges);
        }
    }

    // Determine the new format
    let (new_format, description) = if current_version.is_native() {
        (
            "3.0 (native)".to_string(),
            "Upgrade to newer source format 3.0 (native).".to_string(),
        )
    } else {
        // For non-native packages, check if there's a non-standard patches directory
        let patches_dir = find_patches_directory(base_path)?;
        if let Some(ref dir) = patches_dir {
            if dir != &PathBuf::from("debian/patches") {
                // Non-standard patches directory - don't make changes
                log::warn!("Tree has non-standard patches directory {:?}.", dir);
                return Err(FixerError::NoChanges);
            }
        }

        // Default to 3.0 (quilt)
        // TODO: In the future, we could use breezy to check for non-quilt changes
        // and set single-debian-patch if needed
        (
            "3.0 (quilt)".to_string(),
            "Upgrade to newer source format 3.0 (quilt).".to_string(),
        )
    };

    // Create debian/source directory if it doesn't exist
    if !source_dir.exists() {
        fs::create_dir_all(&source_dir)?;
    }

    // Write the new format
    fs::write(&format_path, format!("{}\n", new_format))?;

    // Report the appropriate tags
    let mut result = FixerResult::builder(description);
    if orig_format.is_none() {
        // If the file was missing, we've fixed both missing-debian-source-format
        // and older-source-format (since missing implies old format 1.0)
        result = result
            .fixed_tag("missing-debian-source-format")
            .fixed_tag("older-source-format");
    } else {
        // If the file existed with "1.0", we only fixed older-source-format
        result = result.fixed_tag("older-source-format");
    }

    Ok(result.build())
}

fn is_debcargo_package(base_path: &Path) -> Result<bool, FixerError> {
    // Check if debian/debcargo.toml exists
    let debcargo_toml = base_path.join("debian/debcargo.toml");
    Ok(debcargo_toml.exists())
}

fn find_patches_directory(base_path: &Path) -> Result<Option<PathBuf>, FixerError> {
    let rules_path = base_path.join("debian/rules");
    if !rules_path.exists() {
        return Ok(None);
    }

    let rules_content = fs::read_to_string(&rules_path)?;
    let makefile = makefile_lossless::Makefile::read(rules_content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/rules: {}", e)))?;

    Ok(debian_analyzer::patches::rules_find_patches_directory(
        &makefile,
    ))
}

declare_fixer! {
    name: "source-format",
    tags: ["missing-debian-source-format", "older-source-format"],
    apply: |basedir, package, version, preferences| {
        run(basedir, package, version, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_no_changes_if_already_modern() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let format_path = source_dir.join("format");
        fs::write(&format_path, "3.0 (quilt)\n").unwrap();

        let version: Version = "1.0-1".parse().unwrap();
        let preferences = FixerPreferences::default();
        let result = run(base_path, "test", &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_upgrade_from_1_0_non_native() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let format_path = source_dir.join("format");
        fs::write(&format_path, "1.0\n").unwrap();

        let version: Version = "1.0-1".parse().unwrap();
        let preferences = FixerPreferences::default();
        let result = run(base_path, "test", &version, &preferences);
        assert!(result.is_ok());

        let new_format = fs::read_to_string(&format_path).unwrap();
        assert_eq!(new_format, "3.0 (quilt)\n");
    }

    #[test]
    fn test_upgrade_from_1_0_native() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let format_path = source_dir.join("format");
        fs::write(&format_path, "1.0\n").unwrap();

        let version: Version = "1.0".parse().unwrap(); // Native version (no revision)
        let preferences = FixerPreferences::default();
        let result = run(base_path, "test", &version, &preferences);
        assert!(result.is_ok());

        let new_format = fs::read_to_string(&format_path).unwrap();
        assert_eq!(new_format, "3.0 (native)\n");
    }

    #[test]
    fn test_create_missing_format() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let version: Version = "1.0-1".parse().unwrap();
        let preferences = FixerPreferences::default();
        let result = run(base_path, "test", &version, &preferences);
        assert!(result.is_ok());

        let format_path = base_path.join("debian/source/format");
        assert!(format_path.exists());
        let new_format = fs::read_to_string(&format_path).unwrap();
        assert_eq!(new_format, "3.0 (quilt)\n");

        let result_unwrapped = result.unwrap();
        assert!(result_unwrapped
            .description
            .contains("Upgrade to newer source format"));
    }

    #[test]
    fn test_skip_debcargo_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Create debcargo.toml to mark it as a debcargo package
        let debcargo_toml = debian_dir.join("debcargo.toml");
        fs::write(&debcargo_toml, "[package]\n").unwrap();

        let version: Version = "1.0-1".parse().unwrap();
        let preferences = FixerPreferences::default();
        let result = run(base_path, "test", &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_creates_source_directory() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let source_dir = debian_dir.join("source");
        assert!(!source_dir.exists());

        let version: Version = "1.0-1".parse().unwrap();
        let preferences = FixerPreferences::default();
        let result = run(base_path, "test", &version, &preferences);
        assert!(result.is_ok());

        assert!(source_dir.exists());
        assert!(source_dir.is_dir());
    }
}
