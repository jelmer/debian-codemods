use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Version};
use std::path::{Path, PathBuf};

pub fn detect(
    base_path: &Path,
    current_version: &Version,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    if !current_version.is_native() {
        return Ok(Vec::new());
    }
    if !preferences.opinionated.unwrap_or(false) {
        return Ok(Vec::new());
    }

    let key_rel = PathBuf::from("debian/upstream/signing-key.asc");
    let key_abs = base_path.join(&key_rel);
    if !key_abs.exists() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "public-upstream-key-in-native-package",
        vec!["[debian/upstream/signing-key.asc]".to_string()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Remove upstream signing key in native source package.",
        vec![
            Action::Filesystem(FilesystemAction::Delete { file: key_rel }),
            Action::Filesystem(FilesystemAction::RemoveDirIfEmpty {
                file: PathBuf::from("debian/upstream"),
            }),
        ],
    )
    .with_certainty(Certainty::Certain)])
}

declare_fixer! {
    name: "public-upstream-key-in-native-package",
    tags: ["public-upstream-key-in-native-package"],
    diagnose: |basedir, _package, version, preferences| {
        detect(basedir, version, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use std::str::FromStr;
    use tempfile::TempDir;

    fn run_apply(
        base: &Path,
        version: &str,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let v = Version::from_str(version).unwrap();
        FixerImpl.apply(base, "test", &v, preferences)
    }

    #[test]
    fn test_native_package_with_signing_key() {
        let tmp = TempDir::new().unwrap();
        let upstream = tmp.path().join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();
        let key = upstream.join("signing-key.asc");
        fs::write(&key, "-----BEGIN PGP PUBLIC KEY BLOCK-----\n").unwrap();

        let prefs = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };
        let result = run_apply(tmp.path(), "1.0", &prefs).unwrap();
        assert_eq!(
            result.description,
            "Remove upstream signing key in native source package."
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert!(!key.exists());
        assert!(!upstream.exists());
    }

    #[test]
    fn test_native_package_not_opinionated() {
        let tmp = TempDir::new().unwrap();
        let upstream = tmp.path().join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();
        let key = upstream.join("signing-key.asc");
        fs::write(&key, "-----BEGIN PGP PUBLIC KEY BLOCK-----\n").unwrap();

        let prefs = FixerPreferences {
            opinionated: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), "1.0", &prefs),
            Err(FixerError::NoChanges)
        ));
        assert!(key.exists());
    }

    #[test]
    fn test_non_native_package() {
        let tmp = TempDir::new().unwrap();
        let upstream = tmp.path().join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();
        let key = upstream.join("signing-key.asc");
        fs::write(&key, "-----BEGIN PGP PUBLIC KEY BLOCK-----\n").unwrap();

        let prefs = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), "1.0-1", &prefs),
            Err(FixerError::NoChanges)
        ));
        assert!(key.exists());
    }

    #[test]
    fn test_no_signing_key_file() {
        let tmp = TempDir::new().unwrap();
        let prefs = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), "1.0", &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_upstream_dir_with_other_files() {
        let tmp = TempDir::new().unwrap();
        let upstream = tmp.path().join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();
        let key = upstream.join("signing-key.asc");
        fs::write(&key, "-----BEGIN PGP PUBLIC KEY BLOCK-----\n").unwrap();
        let other = upstream.join("metadata");
        fs::write(&other, "Name: test\n").unwrap();

        let prefs = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };
        run_apply(tmp.path(), "1.0", &prefs).unwrap();
        assert!(!key.exists());
        assert!(upstream.exists());
        assert!(other.exists());
    }
}
