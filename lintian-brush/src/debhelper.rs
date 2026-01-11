//! Debhelper utility functions.

use std::path::Path;

/// Build steps that debhelper handles
pub const DEBHELPER_BUILD_STEPS: &[&str] = &["configure", "build", "test", "install", "clean"];

/// Error type for buildsystem detection
#[derive(Debug)]
pub enum BuildsystemDetectionError {
    /// dh_assistant command not found or failed to execute
    DhAssistantNotAvailable(std::io::Error),
    /// dh_assistant returned non-zero exit status
    DhAssistantFailed {
        /// Exit status of the command
        status: std::process::ExitStatus,
        /// Standard error output
        stderr: String,
    },
    /// dh_assistant output could not be parsed as JSON
    InvalidJson {
        /// The JSON parsing error
        error: serde_json::Error,
        /// The output that failed to parse
        output: String,
    },
    /// Other I/O error
    IoError(std::io::Error),
}

impl std::fmt::Display for BuildsystemDetectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DhAssistantNotAvailable(e) => write!(f, "dh_assistant not available: {}", e),
            Self::DhAssistantFailed { status, stderr } => {
                write!(f, "dh_assistant failed with status {}: {}", status, stderr)
            }
            Self::InvalidJson { error, output } => {
                write!(
                    f,
                    "Failed to parse dh_assistant output as JSON: {}\nOutput: {}",
                    error, output
                )
            }
            Self::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for BuildsystemDetectionError {}

impl From<std::io::Error> for BuildsystemDetectionError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

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
) -> Result<Option<String>, BuildsystemDetectionError> {
    // Check for autoconf
    if base_path.join("configure.ac").exists() || base_path.join("configure.in").exists() {
        return Ok(Some("autoconf".to_string()));
    }

    // Use dh_assistant to detect the build system
    // Clear any DH_* environment variables that might affect detection
    let mut cmd = std::process::Command::new("dh_assistant");
    cmd.arg("which-build-system");

    // Remove all DH_* environment variables to avoid inheriting build-time settings
    for (key, _) in std::env::vars() {
        if key.starts_with("DH_") {
            cmd.env_remove(&key);
        }
    }

    cmd.env("DH_NO_ACT", "1"); // Prevent dh_assistant from writing .debhelper files
    cmd.current_dir(base_path);

    let output = match cmd.output() {
        Ok(output) => output,
        Err(e) => {
            tracing::debug!("Failed to execute dh_assistant: {}", e);
            return Err(BuildsystemDetectionError::DhAssistantNotAvailable(e));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        tracing::debug!(
            "dh_assistant which-build-system failed with status {}: {}",
            output.status,
            stderr
        );
        return Err(BuildsystemDetectionError::DhAssistantFailed {
            status: output.status,
            stderr,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(parsed) => parsed,
        Err(e) => {
            tracing::debug!(
                "Failed to parse dh_assistant output as JSON: {}\nOutput: {}",
                e,
                stdout
            );
            return Err(BuildsystemDetectionError::InvalidJson {
                error: e,
                output: stdout.to_string(),
            });
        }
    };

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
