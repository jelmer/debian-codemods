use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, DebcargoAction, Diagnostic};
use crate::{Certainty, FixerError, FixerPreferences};
use debian_workspace::Workspace;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let debcargo_rel = PathBuf::from("debian/debcargo.toml");

    let Some(doc) = ws.parsed_debcargo()? else {
        return Ok(Vec::new());
    };

    // If collapse_features is already set to true, nothing to do.
    if doc
        .get("collapse_features")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Ok(Vec::new());
    }

    Ok(vec![Diagnostic {
        issue: None,
        message: "Set collapse_features = true in debian/debcargo.toml.".to_string(),
        certainty: Some(Certainty::Certain),
        patch_name: None,
        plans: vec![ActionPlan {
            label: "Set collapse_features = true in debian/debcargo.toml.".to_string(),
            opinionated: false,
            certainty: None,
            actions: vec![Action::Debcargo(DebcargoAction::SetTopLevelBool {
                file: debcargo_rel,
                field: "collapse_features".to_string(),
                value: true,
            })],
        }],
    }])
}

declare_detector! {
    name: "debcargo-collapse-features",
    tags: [],
    triggers: [
        debian_workspace::Trigger::DebcargoField("collapse_features"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &std::path::Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(v.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_no_debcargo_toml() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("debian")).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_collapse_features_already_true() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("debcargo.toml"), "collapse_features = true\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_collapse_features_missing() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let debcargo_path = debian.join("debcargo.toml");
        fs::write(
            &debcargo_path,
            "[source]\nhomepage = \"https://example.com\"\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Set collapse_features = true in debian/debcargo.toml."
        );
        assert_eq!(
            fs::read_to_string(&debcargo_path).unwrap(),
            "collapse_features = true\n[source]\nhomepage = \"https://example.com\"\n",
        );
    }

    #[test]
    fn test_collapse_features_false() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let debcargo_path = debian.join("debcargo.toml");
        fs::write(&debcargo_path, "collapse_features = false\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Set collapse_features = true in debian/debcargo.toml."
        );
        assert_eq!(
            fs::read_to_string(&debcargo_path).unwrap(),
            "collapse_features = true\n",
        );
    }
}
