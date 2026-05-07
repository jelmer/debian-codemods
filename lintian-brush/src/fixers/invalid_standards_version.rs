use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType};
use debian_analyzer::lintian::StandardsVersion;
use std::path::PathBuf;

const SEP: char = '\t';

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let standards_versions_iter = match debian_analyzer::lintian::iter_standards_versions_opt() {
        Some(iter) => iter,
        None => return Ok(Vec::new()),
    };
    let valid_versions: Vec<StandardsVersion> = standards_versions_iter
        .map(|release| release.version)
        .collect();
    if valid_versions.is_empty() {
        return Ok(Vec::new());
    }

    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let Some(standards_version_str) = source.standards_version() else {
        return Ok(Vec::new());
    };
    let Ok(standards_version): Result<StandardsVersion, _> = standards_version_str.parse() else {
        return Ok(Vec::new());
    };

    // Two-component version that's otherwise valid: add ".0" suffix.
    let parts_count = standards_version_str.matches('.').count() + 1;
    if parts_count == 2 && valid_versions.contains(&standards_version) {
        let issue = LintianIssue {
            package: None,
            package_type: Some(PackageType::Source),
            tag: Some("invalid-standards-version".to_string()),
            info: Some(standards_version_str.clone()),
        };
        let new_value = format!("{}.0", standards_version_str);
        return Ok(vec![Diagnostic::with_actions(
            issue,
            "Standards-Version is missing a .0 suffix.",
            format!("suffix{}{}", SEP, new_value),
            vec![Action::Deb822(Deb822Action::SetField {
                file: control_rel,
                paragraph: ParagraphSelector::Source,
                field: "Standards-Version".into(),
                value: new_value,
            })],
        )]);
    }

    if valid_versions.contains(&standards_version) {
        return Ok(Vec::new());
    }

    let latest_known = valid_versions.iter().max().unwrap();
    if &standards_version > latest_known {
        return Ok(Vec::new());
    }

    let candidates: Vec<_> = valid_versions
        .iter()
        .filter(|v| **v < standards_version)
        .collect();
    let Some(new_version) = candidates.iter().max() else {
        return Ok(Vec::new());
    };
    let new_version_str = new_version.to_string();

    let issue = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        tag: Some("invalid-standards-version".to_string()),
        info: Some(standards_version_str.clone()),
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Standards-Version is invalid.",
        format!(
            "replace{}{}{}{}",
            SEP, standards_version_str, SEP, new_version_str
        ),
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Standards-Version".into(),
            value: new_version_str,
        })],
    )])
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let Some((first, _)) = fixed.first() else {
        return "Update Standards-Version.".to_string();
    };
    let Some(plan) = first.plans.first() else {
        return "Update Standards-Version.".to_string();
    };
    let parts: Vec<&str> = plan.label.split(SEP).collect();
    match parts.first().copied() {
        Some("suffix") => "Add missing .0 suffix in Standards-Version.".to_string(),
        Some("replace") if parts.len() == 3 => format!(
            "Replace invalid standards version {} with valid {}.",
            parts[1], parts[2]
        ),
        _ => "Update Standards-Version.".to_string(),
    }
}

declare_detector! {
    name: "invalid-standards-version",
    tags: ["invalid-standards-version"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Standards-Version",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_parse() {
        assert!("4.6.2".parse::<StandardsVersion>().is_ok());
        assert!("4.6".parse::<StandardsVersion>().is_ok());
        assert!("3.9.8".parse::<StandardsVersion>().is_ok());
        assert!("invalid".parse::<StandardsVersion>().is_err());
    }

    #[test]
    fn test_no_change_when_valid() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nStandards-Version: 4.6.2\n\nPackage: blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_standards_version() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), "Source: blah\n\nPackage: blah\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
