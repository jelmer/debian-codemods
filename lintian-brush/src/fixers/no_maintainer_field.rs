use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_changelog::get_maintainer_from_env;
use debian_workspace::Workspace;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    if source.as_deb822().contains_key("Maintainer") {
        return Ok(Vec::new());
    }

    let get_env = |name: &str| {
        preferences
            .extra_env
            .as_ref()
            .and_then(|e| e.get(name).cloned())
            .or_else(|| std::env::var(name).ok())
    };
    let Some((fullname, email)) = get_maintainer_from_env(get_env) else {
        return Err(FixerError::Other(
            "Could not determine maintainer from environment".to_string(),
        ));
    };
    let maintainer_value = format!("{} <{}>", fullname, email);

    let issue = LintianIssue::source_with_info(
        "required-field",
        Visibility::Error,
        vec!["debian/control Maintainer".to_string()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Maintainer field is missing.",
        format!("Set the maintainer field to: {}.", maintainer_value),
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Maintainer".into(),
            value: maintainer_value,
        })],
    )
    .with_certainty(Certainty::Possible)])
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let Some((first, _)) = fixed.first() else {
        return "Set the maintainer field.".to_string();
    };
    let Some(plan) = first.plans.first() else {
        return "Set the maintainer field.".to_string();
    };
    plan.label.clone()
}

declare_detector! {
    name: "no-maintainer-field",
    tags: ["required-field"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Maintainer",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_maintainer_already_exists() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test\nMaintainer: Existing User <existing@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
