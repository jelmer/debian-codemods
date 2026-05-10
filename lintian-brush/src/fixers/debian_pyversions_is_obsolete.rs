use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use debian_workspace::Workspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let pyversions = PathBuf::from("debian/pyversions");
    let Some(bytes) = ws.read_file(Path::new("debian/pyversions"))? else {
        return Ok(Vec::new());
    };
    let content = std::str::from_utf8(&bytes).map_err(|_| FixerError::NoChanges)?;
    if !content.trim().starts_with("2.") {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "debian-pyversions-is-obsolete",
        Visibility::Info,
        vec!["debian/pyversions".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "debian/pyversions is obsolete.",
        "Remove obsolete debian/pyversions.",
        vec![Action::Filesystem(FilesystemAction::Delete {
            file: pyversions,
        })],
    )])
}

declare_detector! {
    name: "debian-pyversions-is-obsolete",
    tags: ["debian-pyversions-is-obsolete"],
    triggers: [debian_workspace::Trigger::File("debian/pyversions")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test-package", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_obsolete_pyversions() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let pyversions_path = debian_dir.join("pyversions");
        fs::write(&pyversions_path, "2.6-\n").unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(result.description, "Remove obsolete debian/pyversions.");
        assert!(!pyversions_path.exists());
    }

    #[test]
    fn test_no_change_when_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_change_when_not_python2() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let pyversions_path = debian_dir.join("pyversions");
        fs::write(&pyversions_path, "3.6-\n").unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&pyversions_path).unwrap(), "3.6-\n");
    }

    #[test]
    fn test_no_change_when_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let pyversions_path = debian_dir.join("pyversions");
        fs::write(&pyversions_path, "").unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert!(pyversions_path.exists());
    }
}
