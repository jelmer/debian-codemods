use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rel = Path::new("debian/files");
    let contents = match ws.read_file(rel)? {
        Some(c) => c,
        None => return Ok(Vec::new()),
    };

    // lintian only emits the tag for a non-empty debian/files.
    if contents.is_empty() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "debian-files-list-in-source",
        Visibility::Error,
        vec!["debian/files".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "debian/files is present in the source tree.".to_string(),
        "Remove debian/files from the source tree.".to_string(),
        vec![Action::Filesystem(FilesystemAction::Delete {
            file: PathBuf::from("debian/files"),
        })],
    )])
}

declare_detector! {
    name: "debian-files-list-in-source",
    tags: ["debian-files-list-in-source"],
    triggers: [
        debian_workspace::Trigger::File("debian/files"),
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
            Some(version),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn detect_issues(base: &Path) -> Vec<Diagnostic> {
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        detect(&ws, &FixerPreferences::default()).unwrap()
    }

    #[test]
    fn test_removes_debian_files() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let files = debian.join("files");
        fs::write(&files, "foo_1.0-1_amd64.deb misc optional\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert!(!files.exists());
        assert_eq!(
            result.description,
            "Remove debian/files from the source tree."
        );
    }

    #[test]
    fn test_issue_info_matches_lintian() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("files"), "pkg_1_all.deb misc optional\n").unwrap();

        let diags = detect_issues(tmp.path());
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert_eq!(issue.tag.as_deref(), Some("debian-files-list-in-source"));
        assert_eq!(issue.info.as_deref(), Some("debian/files"));
        assert_eq!(
            issue.to_string(),
            "source: debian-files-list-in-source debian/files"
        );
    }

    #[test]
    fn test_no_change_when_no_debian_files() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), "").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_empty_debian_files() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let files = debian.join("files");
        fs::write(&files, "").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        // An empty debian/files does not trigger the tag, so it is left alone.
        assert!(files.exists());
    }
}
