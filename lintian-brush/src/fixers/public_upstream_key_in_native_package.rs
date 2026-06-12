use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let Some(current_version) = ws.current_version() else {
        return Ok(Vec::new());
    };
    if !current_version.is_native() {
        return Ok(Vec::new());
    }

    let key_rel = PathBuf::from("debian/upstream/signing-key.asc");
    if ws.read_file(&key_rel)?.is_none() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "public-upstream-key-in-native-package",
        Visibility::Info,
        vec!["[debian/upstream/signing-key.asc]".to_string()],
    );
    Ok(vec![Diagnostic {
        issue: Some(issue),
        message: "Remove upstream signing key in native source package.".to_string(),
        certainty: Some(Certainty::Certain),
        patch_name: None,
        plans: vec![ActionPlan {
            label: "Remove upstream signing key in native source package.".to_string(),
            opinionated: true,
            certainty: None,
            actions: vec![
                Action::Filesystem(FilesystemAction::Delete { file: key_rel }),
                Action::Filesystem(FilesystemAction::RemoveDirIfEmpty {
                    file: PathBuf::from("debian/upstream"),
                }),
            ],
        }],
    }])
}

declare_detector! {
    name: "public-upstream-key-in-native-package",
    tags: ["public-upstream-key-in-native-package"],
    triggers: [
        debian_workspace::Trigger::Changelog(debian_workspace::ChangelogAspect::Version),
        debian_workspace::Trigger::File("debian/upstream/signing-key.asc"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use std::str::FromStr;
    use tempfile::TempDir;

    fn run_apply(
        base: &Path,
        version: &str,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let v = Version::from_str(version).unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(v.clone()),
            );
            adapter.apply(&ws, preferences)
        }
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
