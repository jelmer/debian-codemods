use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rel = PathBuf::from("debian/tests/control");
    let bytes = match ws.read_file(&rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };
    if !content.trim().is_empty() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "empty-debian-tests-control",
        vec!["[debian/tests/control]".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Remove empty debian/tests/control.",
        vec![
            Action::Filesystem(FilesystemAction::Delete { file: rel }),
            Action::Filesystem(FilesystemAction::RemoveDirIfEmpty {
                file: PathBuf::from("debian/tests"),
            }),
        ],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "empty-debian-tests-control",
    tags: ["empty-debian-tests-control"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_empty_tests_control() {
        let tmp = TempDir::new().unwrap();
        let tests_dir = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();
        fs::write(tests_dir.join("control"), "").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, "Remove empty debian/tests/control.");
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert!(!tests_dir.join("control").exists());
        assert!(!tests_dir.exists());
    }

    #[test]
    fn test_remove_whitespace_only_tests_control() {
        let tmp = TempDir::new().unwrap();
        let tests_dir = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();
        fs::write(tests_dir.join("control"), "   \n\t  \n  ").unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(!tests_dir.join("control").exists());
        assert!(!tests_dir.exists());
    }

    #[test]
    fn test_keep_non_empty_tests_control() {
        let tmp = TempDir::new().unwrap();
        let tests_dir = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();
        fs::write(tests_dir.join("control"), "Tests: autopkgtest\nDepends: @").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert!(tests_dir.join("control").exists());
    }

    #[test]
    fn test_keep_tests_dir_with_other_files() {
        let tmp = TempDir::new().unwrap();
        let tests_dir = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();
        fs::write(tests_dir.join("control"), "").unwrap();
        fs::write(tests_dir.join("other-test"), "some content").unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(!tests_dir.join("control").exists());
        assert!(tests_dir.exists());
        assert!(tests_dir.join("other-test").exists());
    }

    #[test]
    fn test_no_tests_control_file() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("debian")).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
