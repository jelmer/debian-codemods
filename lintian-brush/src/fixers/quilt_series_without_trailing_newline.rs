use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction, TextRange};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rel = PathBuf::from("debian/patches/series");
    let content = match ws.read_file(&rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    if content.is_empty() {
        return Ok(Vec::new());
    }
    if content[content.len() - 1] == b'\n' {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "quilt-series-without-trailing-newline",
        Visibility::Error,
        vec!["debian/patches/series".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "debian/patches/series is missing a trailing newline.",
        "Add missing trailing newline in debian/patches/series.",
        vec![Action::Filesystem(FilesystemAction::ReplaceText {
            file: rel,
            range: TextRange {
                start: content.len(),
                end: content.len(),
            },
            replacement: "\n".into(),
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "quilt-series-without-trailing-newline",
    tags: ["quilt-series-without-trailing-newline"],
    triggers: [
        debian_workspace::Trigger::File("debian/patches/series"),
    ],
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
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_adds_missing_newline() {
        let tmp = TempDir::new().unwrap();
        let patches_dir = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        let series = patches_dir.join("series");
        fs::write(&series, b"patch1.diff").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(fs::read(&series).unwrap(), b"patch1.diff\n");
    }

    #[test]
    fn test_no_change_when_newline_exists() {
        let tmp = TempDir::new().unwrap();
        let patches_dir = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        fs::write(patches_dir.join("series"), b"patch1.diff\n").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_empty() {
        let tmp = TempDir::new().unwrap();
        let patches_dir = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        fs::write(patches_dir.join("series"), b"").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_series_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
