use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let old_rel = PathBuf::from("debian/source.lintian-overrides");
    let new_rel = PathBuf::from("debian/source/lintian-overrides");

    let Some(old_bytes) = ws.read_file(&old_rel)? else {
        return Ok(Vec::new());
    };
    let old_content = String::from_utf8(old_bytes).map_err(|e| FixerError::Other(e.to_string()))?;

    let merged_content = if let Some(existing_bytes) = ws.read_file(&new_rel)? {
        let mut existing =
            String::from_utf8(existing_bytes).map_err(|e| FixerError::Other(e.to_string()))?;
        existing.push_str(&old_content);
        existing
    } else {
        old_content
    };

    let issue = LintianIssue::source_with_info(
        "old-source-override-location",
        vec!["debian/source.lintian-overrides".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Move source package lintian overrides to debian/source.",
        vec![
            Action::Filesystem(FilesystemAction::Write {
                file: new_rel,
                content: merged_content.into_bytes(),
            }),
            Action::Filesystem(FilesystemAction::Delete { file: old_rel }),
        ],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "package-uses-deprecated-source-override-location",
    tags: ["old-source-override-location"],
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
    fn test_simple_move() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let old_path = debian_dir.join("source.lintian-overrides");
        fs::write(&old_path, "foo source: some-tag exact match\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Move source package lintian overrides to debian/source."
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert!(!old_path.exists());
        let new_path = debian_dir.join("source/lintian-overrides");
        assert_eq!(
            fs::read_to_string(&new_path).unwrap(),
            "foo source: some-tag exact match\n"
        );
    }

    #[test]
    fn test_append_to_existing() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let old_path = debian_dir.join("source.lintian-overrides");
        let new_path = source_dir.join("lintian-overrides");
        fs::write(&old_path, "foo source: tag-a\n").unwrap();
        fs::write(&new_path, "foo source: tag-b\n").unwrap();

        run_apply(base_path).unwrap();

        assert!(!old_path.exists());
        assert_eq!(
            fs::read_to_string(&new_path).unwrap(),
            "foo source: tag-b\nfoo source: tag-a\n"
        );
    }

    #[test]
    fn test_no_change_when_no_old_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }
}
