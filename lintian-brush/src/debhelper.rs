//! Debhelper utility functions.

use std::path::Path;

/// Build steps that debhelper handles
pub const DEBHELPER_BUILD_STEPS: &[&str] = &["configure", "build", "test", "install", "clean"];

/// Detect the build system for debhelper.
///
/// # Arguments
/// * `base_path` - The base path of the package
/// * `step` - Optional step to determine the buildsystem for (currently unused, for future compatibility)
///
/// # Returns
/// Build system name or None if none could be found
pub fn detect_debhelper_buildsystem(
    base_path: &Path,
    _step: Option<&str>,
) -> Result<Option<String>, std::io::Error> {
    // Check for autoconf
    if base_path.join("configure.ac").exists() || base_path.join("configure.in").exists() {
        return Ok(Some("autoconf".to_string()));
    }

    // Use dh_assistant to detect the build system
    let output = std::process::Command::new("dh_assistant")
        .arg("which-build-system")
        .env("DH_NO_ACT", "1") // Prevent dh_assistant from writing .debhelper files
        .current_dir(base_path)
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    Ok(parsed
        .get("build-system")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debhelper_build_steps() {
        assert_eq!(DEBHELPER_BUILD_STEPS.len(), 5);
        assert!(DEBHELPER_BUILD_STEPS.contains(&"configure"));
        assert!(DEBHELPER_BUILD_STEPS.contains(&"build"));
        assert!(DEBHELPER_BUILD_STEPS.contains(&"test"));
        assert!(DEBHELPER_BUILD_STEPS.contains(&"install"));
        assert!(DEBHELPER_BUILD_STEPS.contains(&"clean"));
    }

    #[test]
    fn test_detect_autoconf() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create a configure.ac file
        fs::write(base_path.join("configure.ac"), "AC_INIT\n").unwrap();

        let result = detect_debhelper_buildsystem(base_path, None).unwrap();
        assert_eq!(result, Some("autoconf".to_string()));
    }

    #[test]
    fn test_detect_autoconf_configure_in() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create a configure.in file
        fs::write(base_path.join("configure.in"), "AC_INIT\n").unwrap();

        let result = detect_debhelper_buildsystem(base_path, None).unwrap();
        assert_eq!(result, Some("autoconf".to_string()));
    }
}
