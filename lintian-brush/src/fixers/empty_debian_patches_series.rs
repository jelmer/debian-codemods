use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, FixerPreferences};
use debian_workspace::Workspace;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rel = PathBuf::from("debian/patches/series");
    let bytes = match ws.read_file(&rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };
    if !content.trim().is_empty() {
        return Ok(Vec::new());
    }

    Ok(vec![Diagnostic {
        issue: None,
        message: "Remove empty debian/patches/series.".to_string(),
        certainty: Some(Certainty::Certain),
        patch_name: None,
        plans: vec![ActionPlan {
            label: "Remove empty debian/patches/series.".to_string(),
            opinionated: true,
            certainty: None,
            actions: vec![Action::Filesystem(FilesystemAction::Delete { file: rel })],
        }],
    }])
}

declare_detector! {
    name: "empty-debian-patches-series",
    tags: [],
    triggers: [debian_workspace::Trigger::File("debian/patches/series")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path, opinionated: bool) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let preferences = FixerPreferences {
            opinionated: Some(opinionated),
            ..Default::default()
        };
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &preferences)
        }
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
