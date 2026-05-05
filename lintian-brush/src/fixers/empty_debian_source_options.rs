use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError};
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let rel = PathBuf::from("debian/source/options");
    let abs = base_path.join(&rel);
    if !abs.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&abs)?;
    if !content.trim().is_empty() {
        return Ok(Vec::new());
    }

    Ok(vec![Diagnostic::untagged(
        "Remove empty debian/source/options.",
        vec![Action::Filesystem(FilesystemAction::Delete { file: rel })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_fixer! {
    name: "empty-debian-source-options",
    tags: [],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_empty_options() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let options_path = source_dir.join("options");
        fs::write(&options_path, "").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, "Remove empty debian/source/options.");
        assert!(!options_path.exists());
    }

    #[test]
    fn test_remove_whitespace_only_options() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let options_path = source_dir.join("options");
        fs::write(&options_path, "   \n\t  \n  ").unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(!options_path.exists());
    }

    #[test]
    fn test_keep_non_empty_options() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let options_path = source_dir.join("options");
        fs::write(&options_path, "compression = xz\n").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert!(options_path.exists());
    }

    #[test]
    fn test_no_options_file() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("debian/source")).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
