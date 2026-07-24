use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

/// Return `Some(installed_name)` if `filename` is a debhelper tmpfiles source
/// file, where `installed_name` is the basename the file gets under
/// `usr/lib/tmpfiles.d/`.
///
/// dh_installtmpfiles takes `debian/<pkg>.tmpfiles` (or the deprecated
/// `debian/<pkg>.tmpfile`) and installs it as `usr/lib/tmpfiles.d/<pkg>.conf`.
fn tmpfiles_installed_name(filename: &str) -> Option<String> {
    if let Some(pkg) = filename.strip_suffix(".tmpfiles") {
        return Some(format!("{}.conf", pkg));
    }
    if let Some(pkg) = filename.strip_suffix(".tmpfile") {
        return Some(format!("{}.conf", pkg));
    }
    None
}

/// Check whether any non-comment line in `content` declares a `d` (or `D`)
/// tmpfiles.d entry rooted at `/var/run/`.
///
/// Lintian's check fires when the file contains such a line, ignoring the
/// rest of the fields - see Lintian::Check::Systemd::Tmpfiles.
fn has_var_run_directory_entry(content: &str) -> bool {
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Tmpfiles entries are whitespace-separated; the first field is the
        // type (with optional modifier chars like `!`, `+`, `-`, `=`) and the
        // second field is the path. Lintian only flags `d`, but we handle any
        // directory-creating type that names /var/run so the fix is complete.
        let mut fields = trimmed.split_whitespace();
        let Some(type_field) = fields.next() else {
            continue;
        };
        let core = type_field.trim_start_matches(|c: char| "!+-=".contains(c));
        if core != "d" && core != "D" {
            continue;
        }
        let Some(path) = fields.next() else {
            continue;
        };
        if path.starts_with("/var/run/") || path == "/var/run" {
            return true;
        }
    }
    false
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut entries = match ws.list_dir(Path::new("debian"))? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    entries.sort();

    let mut diagnostics = Vec::new();

    for filename in entries {
        let Some(installed_name) = tmpfiles_installed_name(&filename) else {
            continue;
        };
        let rel = PathBuf::from("debian").join(&filename);
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if !has_var_run_directory_entry(content) {
            continue;
        }

        let installed_path = format!("usr/lib/tmpfiles.d/{}", installed_name);
        let issue = LintianIssue::source_with_info(
            "systemd-tmpfile-in-var-run",
            Visibility::Info,
            vec![format!("[{}]", installed_path)],
        );

        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!(
                "systemd tmpfiles.d entry in {} declares a path under /var/run.",
                rel.display()
            ),
            "Replace /var/run with /run in systemd tmpfiles.d configuration.",
            vec![Action::Filesystem(FilesystemAction::Substitute {
                file: rel.clone(),
                from: "/var/run/".into(),
                to: "/run/".into(),
            })],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "systemd-tmpfile-in-var-run",
    tags: ["systemd-tmpfile-in-var-run"],
    triggers: [
        debian_workspace::Trigger::Glob("debian/*.tmpfiles"),
        debian_workspace::Trigger::Glob("debian/*.tmpfile"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version.clone()),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    #[test]
    fn test_tmpfiles_installed_name() {
        assert_eq!(
            tmpfiles_installed_name("foo.tmpfiles").as_deref(),
            Some("foo.conf")
        );
        assert_eq!(
            tmpfiles_installed_name("foo.tmpfile").as_deref(),
            Some("foo.conf")
        );
        assert_eq!(tmpfiles_installed_name("foo.service"), None);
        assert_eq!(tmpfiles_installed_name("control"), None);
    }

    #[test]
    fn test_has_var_run_directory_entry_positive() {
        assert!(has_var_run_directory_entry(
            "d /var/run/foo 0755 root root -\n"
        ));
        assert!(has_var_run_directory_entry(
            "D /var/run/foo 0755 root root -\n"
        ));
        // Leading whitespace is tolerated.
        assert!(has_var_run_directory_entry(
            "  d /var/run/foo 0755 root root -\n"
        ));
    }

    #[test]
    fn test_has_var_run_directory_entry_negative() {
        // Only comments.
        assert!(!has_var_run_directory_entry(
            "# d /var/run/foo 0755 root root -\n"
        ));
        // Already using /run.
        assert!(!has_var_run_directory_entry(
            "d /run/foo 0755 root root -\n"
        ));
        // Different type (file, not directory).
        assert!(!has_var_run_directory_entry(
            "f /var/run/foo 0755 root root -\n"
        ));
        // /var/runfoo - not under /var/run.
        assert!(!has_var_run_directory_entry(
            "d /var/runfoo 0755 root root -\n"
        ));
        // Empty file.
        assert!(!has_var_run_directory_entry(""));
    }

    #[test]
    fn test_replace_var_run() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.tmpfiles");
        fs::write(&path, "d /var/run/foo 0755 root root -\n").unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "d /run/foo 0755 root root -\n",
        );
    }

    #[test]
    fn test_deprecated_tmpfile_extension() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.tmpfile");
        fs::write(&path, "d /var/run/foo 0755 root root -\n").unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "d /run/foo 0755 root root -\n",
        );
    }

    #[test]
    fn test_no_var_run_unchanged() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.tmpfiles");
        let original = "d /run/foo 0755 root root -\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_comment_only_var_run_left_alone() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.tmpfiles");
        let original = "# d /var/run/foo 0755 root root -\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_tmpfiles_files() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("control"), "Source: test\n").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_entries_all_rewritten() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.tmpfiles");
        fs::write(
            &path,
            "d /var/run/foo 0755 root root -\nd /var/run/foo/bar 0755 root root -\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "d /run/foo 0755 root root -\nd /run/foo/bar 0755 root root -\n",
        );
    }
}
