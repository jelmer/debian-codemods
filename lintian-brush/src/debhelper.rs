//! Debhelper utility functions.

use debian_workspace::Workspace;
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

/// Whether debhelper >= 13 substitutes `${var}` in its config files itself,
/// so that a reference to it is not a reason to use dh-exec.
pub fn is_native_subst_var(var: &str) -> bool {
    // DEB_BUILD_*, DEB_HOST_* and DEB_TARGET_* with the well-known suffixes.
    let Some(rest) = var
        .strip_prefix("DEB_BUILD_")
        .or_else(|| var.strip_prefix("DEB_HOST_"))
        .or_else(|| var.strip_prefix("DEB_TARGET_"))
    else {
        return false;
    };
    matches!(
        rest,
        "ARCH"
            | "ARCH_OS"
            | "ARCH_CPU"
            | "ARCH_BITS"
            | "ARCH_ENDIAN"
            | "GNU_CPU"
            | "GNU_SYSTEM"
            | "GNU_TYPE"
            | "MULTIARCH"
    )
}

/// Whether `line` uses the dh-exec `src => dest` rename form.
///
/// Matches lintian's `[ \t]=>[ \t]` pattern: one whitespace character on
/// each side of `=>`.
pub fn has_dh_exec_rename(line: &str) -> bool {
    if let Some(idx) = line.find("=>") {
        let before = line[..idx].chars().last();
        let after = line[idx + 2..].chars().next();
        matches!(before, Some(' ' | '\t')) && matches!(after, Some(' ' | '\t'))
    } else {
        false
    }
}

/// Whether `line` has a dh-exec `[arch]` architecture filter or `<profile>`
/// build-profile filter.
pub fn has_dh_exec_filter(line: &str) -> bool {
    contains_balanced(line, '[', ']') || contains_balanced(line, '<', '>')
}

fn contains_balanced(line: &str, open: char, close: char) -> bool {
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c == open {
            let rest: String = it.clone().collect();
            if let Some(close_idx) = rest.find(close) {
                if close_idx > 0 {
                    return true;
                }
            }
        }
    }
    false
}

/// The names of the `${VAR}` substitutions in `line`.
pub fn extract_subst_vars(line: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'$' && bytes[i + 1] == b'{' {
            if let Some(end) = line[i + 2..].find('}') {
                vars.push(line[i + 2..i + 2 + end].to_string());
                i = i + 2 + end + 1;
                continue;
            }
        }
        i += 1;
    }
    vars
}

/// A dh-exec hashbang line, matching lintian's `hashbang =~ /dh-exec/` test.
fn is_dh_exec_shebang(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("#!") else {
        return false;
    };
    // Match lintian: any hashbang that mentions "dh-exec". This covers both
    // `#!/usr/bin/dh-exec` and `#!/usr/bin/env dh-exec`.
    rest.contains("dh-exec")
}

/// Read `path` if it is a debhelper config file that is run through dh-exec.
///
/// Returns the file's mode and its contents after the hashbang line.
pub fn read_dh_exec_script(
    ws: &dyn Workspace,
    path: &Path,
) -> Result<Option<(u32, String)>, debian_workspace::Error> {
    // e.g. debian/examples/ is a directory of example files, not a
    // debhelper config file.
    if ws.is_dir(path)? {
        return Ok(None);
    }
    let Some(mode) = ws.file_mode(path)? else {
        return Ok(None);
    };
    if mode & 0o111 == 0 {
        // Not executable -> debhelper won't treat it as a script.
        return Ok(None);
    }
    let Some(bytes) = ws.read_file(path)? else {
        return Ok(None);
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(None);
    };
    let Some(first_line) = content.split_inclusive('\n').next() else {
        return Ok(None);
    };
    if !is_dh_exec_shebang(first_line) {
        return Ok(None);
    }
    Ok(Some((mode, content[first_line.len()..].to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_dh_exec_shebang() {
        assert!(is_dh_exec_shebang("#!/usr/bin/dh-exec"));
        assert!(is_dh_exec_shebang("#!/usr/bin/env dh-exec"));
        assert!(!is_dh_exec_shebang("#!/bin/sh"));
        assert!(!is_dh_exec_shebang("not a shebang"));
    }

    #[test]
    fn test_is_native_subst_var() {
        assert!(is_native_subst_var("DEB_HOST_MULTIARCH"));
        assert!(is_native_subst_var("DEB_BUILD_GNU_TYPE"));
        assert!(is_native_subst_var("DEB_TARGET_ARCH"));
        assert!(!is_native_subst_var("DEB_HOST_FOO"));
        assert!(!is_native_subst_var("MULTIARCH"));
    }

    #[test]
    fn test_has_dh_exec_rename() {
        assert!(has_dh_exec_rename("foo => usr/bin/bar"));
        assert!(has_dh_exec_rename("foo\t=>\tusr/bin/bar"));
        assert!(!has_dh_exec_rename("foo=>usr/bin/bar"));
        assert!(!has_dh_exec_rename("usr/bin/foo"));
    }

    #[test]
    fn test_has_dh_exec_filter() {
        assert!(has_dh_exec_filter("usr/bin/foo [amd64]"));
        assert!(has_dh_exec_filter("usr/bin/foo <!nodoc>"));
        assert!(!has_dh_exec_filter("usr/bin/foo []"));
        assert!(!has_dh_exec_filter("usr/bin/foo"));
    }

    #[test]
    fn test_extract_subst_vars() {
        assert_eq!(
            extract_subst_vars("usr/lib/${DEB_HOST_MULTIARCH}/${FOO}/bar"),
            vec!["DEB_HOST_MULTIARCH".to_string(), "FOO".to_string()]
        );
        assert_eq!(
            extract_subst_vars("usr/lib/${unterminated"),
            Vec::<String>::new()
        );
        assert_eq!(extract_subst_vars("usr/bin/foo"), Vec::<String>::new());
    }

    fn dh_exec_script(content: &str, mode: u32) -> Option<(u32, String)> {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("debian")).unwrap();
        let path = tmp.path().join("debian/install");
        std::fs::write(&path, content).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(tmp.path(), None, None);
        // The reported mode includes the file type bits.
        read_dh_exec_script(&ws, Path::new("debian/install"))
            .unwrap()
            .map(|(mode, body)| (mode & 0o7777, body))
    }

    #[test]
    fn test_read_dh_exec_script() {
        assert_eq!(
            dh_exec_script("#!/usr/bin/dh-exec\nusr/bin/foo\n", 0o755),
            Some((0o755, "usr/bin/foo\n".to_string()))
        );
        assert_eq!(
            dh_exec_script("#!/usr/bin/dh-exec\r\nusr/bin/foo\r\n", 0o755),
            Some((0o755, "usr/bin/foo\r\n".to_string()))
        );
        assert_eq!(
            dh_exec_script("#!/usr/bin/dh-exec", 0o755),
            Some((0o755, String::new()))
        );
    }

    #[test]
    fn test_read_dh_exec_script_not_executable() {
        assert_eq!(
            dh_exec_script("#!/usr/bin/dh-exec\nusr/bin/foo\n", 0o644),
            None
        );
    }

    #[test]
    fn test_read_dh_exec_script_other_interpreter() {
        assert_eq!(dh_exec_script("#!/bin/sh\necho usr/bin/foo\n", 0o755), None);
        assert_eq!(dh_exec_script("usr/bin/foo\n", 0o755), None);
        assert_eq!(dh_exec_script("", 0o755), None);
    }

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
