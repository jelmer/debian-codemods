use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, LintianIssue};
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let pycompat = PathBuf::from("debian/pycompat");
    if !base_path.join(&pycompat).exists() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "debian-pycompat-is-obsolete",
        vec!["debian/pycompat".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Remove obsolete debian/pycompat file.",
        vec![Action::Filesystem(FilesystemAction::Delete {
            file: pycompat,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_fixer! {
    name: "debian-pycompat-is-obsolete",
    tags: ["debian-pycompat-is-obsolete"],
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
        assert!(detect(base_path).unwrap().is_empty());
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
