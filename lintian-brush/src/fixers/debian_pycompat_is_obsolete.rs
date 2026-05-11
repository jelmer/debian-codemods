use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use debian_workspace::Workspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let pycompat = PathBuf::from("debian/pycompat");
    if ws.read_file(Path::new("debian/pycompat"))?.is_none() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "debian-pycompat-is-obsolete",
        Visibility::Info,
        vec!["debian/pycompat".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "debian/pycompat is obsolete.",
        "Remove obsolete debian/pycompat file.",
        vec![Action::Filesystem(FilesystemAction::Delete {
            file: pycompat,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "debian-pycompat-is-obsolete",
    tags: ["debian-pycompat-is-obsolete"],
    triggers: [debian_workspace::Trigger::File("debian/pycompat")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use debian_workspace::{DetectorAdapter, FsWorkspace};
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, "test", "1.0".parse().unwrap());
        detect(&ws, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_pycompat() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let pycompat_path = debian_dir.join("pycompat");
        fs::write(&pycompat_path, "2.7\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, "Remove obsolete debian/pycompat file.");
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert!(!pycompat_path.exists());
    }

    #[test]
    fn test_no_pycompat() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert!(detect_in(base_path).unwrap().is_empty());
    }

    #[test]
    fn test_no_debian_dir() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
