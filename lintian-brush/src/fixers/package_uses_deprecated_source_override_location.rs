use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, LintianIssue};
use std::fs;
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let old_rel = PathBuf::from("debian/source.lintian-overrides");
    let new_rel = PathBuf::from("debian/source/lintian-overrides");

    let old_abs = base_path.join(&old_rel);
    if !old_abs.exists() {
        return Ok(Vec::new());
    }
    let new_abs = base_path.join(&new_rel);

    let issue = LintianIssue::source_with_info(
        "old-source-override-location",
        vec!["debian/source.lintian-overrides".to_string()],
    );

    // If the target already exists, merge the old file's content into it
    // and delete the old. Otherwise an atomic rename does the job.
    let actions = if new_abs.exists() {
        let old_content = fs::read_to_string(&old_abs)?;
        let mut merged = fs::read_to_string(&new_abs)?;
        merged.push_str(&old_content);
        vec![
            Action::Filesystem(FilesystemAction::Write {
                file: new_rel,
                content: merged.into_bytes(),
            }),
            Action::Filesystem(FilesystemAction::Delete { file: old_rel }),
        ]
    } else {
        vec![Action::Filesystem(FilesystemAction::Rename {
            file: old_rel,
            to: new_rel,
        })]
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Move source package lintian overrides to debian/source.",
        actions,
    )
    .with_certainty(Certainty::Certain)])
}

declare_fixer! {
    name: "package-uses-deprecated-source-override-location",
    tags: ["old-source-override-location"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
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
