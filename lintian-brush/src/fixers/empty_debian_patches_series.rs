use crate::diagnostic::{Action, ActionPlan, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, FixerPreferences};
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let rel = PathBuf::from("debian/patches/series");
    let abs = base_path.join(&rel);
    if !abs.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&abs)?;
    if !content.trim().is_empty() {
        return Ok(Vec::new());
    }

    Ok(vec![Diagnostic {
        issue: None,
        message: "Remove empty debian/patches/series.".to_string(),
        certainty: Some(Certainty::Certain),
        patch_name: None,
        plans: vec![ActionPlan {
            label: None,
            opinionated: true,
            actions: vec![Action::Filesystem(FilesystemAction::Delete { file: rel })],
        }],
    }])
}

declare_fixer! {
    name: "empty-debian-patches-series",
    tags: [],
    diagnose: |basedir, _package, _version, _preferences: &FixerPreferences| {
        detect(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path, opinionated: bool) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let preferences = FixerPreferences {
            opinionated: Some(opinionated),
            ..Default::default()
        };
        FixerImpl.apply(base, "test", &version, &preferences)
    }

    #[test]
    fn test_remove_empty_series_opinionated() {
        let tmp = TempDir::new().unwrap();
        let patches_dir = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        let series_path = patches_dir.join("series");
        fs::write(&series_path, "").unwrap();

        let result = run_apply(tmp.path(), true).unwrap();
        assert_eq!(result.description, "Remove empty debian/patches/series.");
        assert!(!series_path.exists());
    }

    #[test]
    fn test_remove_whitespace_only_series_opinionated() {
        let tmp = TempDir::new().unwrap();
        let patches_dir = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        let series_path = patches_dir.join("series");
        fs::write(&series_path, "   \n\t  \n  ").unwrap();

        run_apply(tmp.path(), true).unwrap();
        assert!(!series_path.exists());
    }

    #[test]
    fn test_not_opinionated() {
        let tmp = TempDir::new().unwrap();
        let patches_dir = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        let series_path = patches_dir.join("series");
        fs::write(&series_path, "").unwrap();

        assert!(matches!(
            run_apply(tmp.path(), false),
            Err(FixerError::NoChanges)
        ));
        assert!(series_path.exists());
    }

    #[test]
    fn test_keep_non_empty_series() {
        let tmp = TempDir::new().unwrap();
        let patches_dir = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        let series_path = patches_dir.join("series");
        fs::write(&series_path, "some-patch.patch\n").unwrap();

        assert!(matches!(
            run_apply(tmp.path(), true),
            Err(FixerError::NoChanges)
        ));
        assert!(series_path.exists());
    }

    #[test]
    fn test_no_series_file() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("debian/patches")).unwrap();
        assert!(matches!(
            run_apply(tmp.path(), true),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(tmp.path(), true),
            Err(FixerError::NoChanges)
        ));
    }
}
