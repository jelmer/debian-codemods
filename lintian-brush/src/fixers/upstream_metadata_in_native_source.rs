use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn FixerWorkspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let Some(current_version) = ws.current_version() else {
        return Ok(Vec::new());
    };
    if !current_version.is_native() {
        return Ok(Vec::new());
    }
    if !preferences.opinionated.unwrap_or(false) {
        return Ok(Vec::new());
    }

    let metadata_rel = PathBuf::from("debian/upstream/metadata");
    if ws
        .read_file(Path::new("debian/upstream/metadata"))?
        .is_none()
    {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "upstream-metadata-in-native-source",
        Visibility::Warning,
        vec!["[debian/upstream/metadata]".to_string()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Native source package contains debian/upstream/metadata.",
        "Remove debian/upstream/metadata in native source package.",
        vec![
            Action::Filesystem(FilesystemAction::Delete { file: metadata_rel }),
            Action::Filesystem(FilesystemAction::RemoveDirIfEmpty {
                file: PathBuf::from("debian/upstream"),
            }),
        ],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "upstream-metadata-in-native-source",
    tags: ["upstream-metadata-in-native-source"],
    triggers: [
        crate::workspace::Trigger::UpstreamMetadataField("*"),
        crate::workspace::Trigger::Changelog(crate::workspace::ChangelogAspect::Version),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::Version;
    use std::fs;
    use std::str::FromStr;
    use tempfile::TempDir;

    fn run_apply(
        base: &Path,
        version: &str,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let v = Version::from_str(version).unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, preferences)
    }

    #[test]
    fn test_native_package_with_metadata() {
        let tmp = TempDir::new().unwrap();
        let upstream = tmp.path().join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();
        let metadata = upstream.join("metadata");
        fs::write(&metadata, "Name: test\n").unwrap();

        let prefs = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };
        let result = run_apply(tmp.path(), "1.0", &prefs).unwrap();
        assert_eq!(
            result.description,
            "Remove debian/upstream/metadata in native source package."
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert!(!metadata.exists());
        assert!(!upstream.exists());
    }

    #[test]
    fn test_native_package_not_opinionated() {
        let tmp = TempDir::new().unwrap();
        let upstream = tmp.path().join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();
        let metadata = upstream.join("metadata");
        fs::write(&metadata, "Name: test\n").unwrap();

        let prefs = FixerPreferences {
            opinionated: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), "1.0", &prefs),
            Err(FixerError::NoChanges)
        ));
        assert!(metadata.exists());
    }

    #[test]
    fn test_non_native_package() {
        let tmp = TempDir::new().unwrap();
        let upstream = tmp.path().join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();
        let metadata = upstream.join("metadata");
        fs::write(&metadata, "Name: test\n").unwrap();

        let prefs = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), "1.0-1", &prefs),
            Err(FixerError::NoChanges)
        ));
        assert!(metadata.exists());
    }

    #[test]
    fn test_no_metadata_file() {
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
        let metadata = upstream.join("metadata");
        fs::write(&metadata, "Name: test\n").unwrap();
        let other = upstream.join("repository");
        fs::write(&other, "https://example.com/repo\n").unwrap();

        let prefs = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };
        run_apply(tmp.path(), "1.0", &prefs).unwrap();
        assert!(!metadata.exists());
        assert!(upstream.exists());
        assert!(other.exists());
    }
}
