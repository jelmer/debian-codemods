use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::collections::HashMap;
use std::path::PathBuf;
use url::Url;

const HOST_TO_VCS: &[(&str, &str)] = &[
    ("github.com", "Git"),
    ("gitlab.com", "Git"),
    ("salsa.debian.org", "Git"),
];

const SEP: char = '\t';

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let Some(source_name) = source.as_deb822().get("Source") else {
        return Ok(Vec::new());
    };

    let host_map: HashMap<&str, &str> = HOST_TO_VCS.iter().copied().collect();
    let para = source.as_deb822();
    let vcs_fields: Vec<String> = para
        .keys()
        .filter(|k| k.starts_with("Vcs-") && k.to_lowercase() != "vcs-browser")
        .map(|k| k.to_string())
        .collect();

    let mut diagnostics = Vec::new();
    for field in vcs_fields {
        let vcs_type = field.strip_prefix("Vcs-").unwrap_or(&field).to_string();
        let Some(vcs_url) = para.get(&field) else {
            continue;
        };
        let Ok(parsed_url) = Url::parse(&vcs_url) else {
            continue;
        };
        let Some(host) = parsed_url.host_str() else {
            continue;
        };
        let clean_host = host.split('@').next_back().unwrap_or(host);
        let Some(&actual_vcs) = host_map.get(clean_host) else {
            continue;
        };
        if actual_vcs == vcs_type {
            continue;
        }

        let issue = LintianIssue::source_with_info(
            "vcs-field-mismatch",
            vec![format!(
                "Vcs-{} != Vcs-{} {}",
                vcs_type, actual_vcs, vcs_url
            )],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("rename{}{}{}{}", SEP, vcs_type, SEP, actual_vcs),
            vec![Action::Deb822(Deb822Action::RenameField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::ByKey {
                    field: "Source".into(),
                    value: source_name.clone(),
                },
                from: field.clone(),
                to: format!("Vcs-{}", actual_vcs),
            })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let Some(first) = fixed.first() else {
        return "Fix Vcs-* type mismatch.".to_string();
    };
    let parts: Vec<&str> = first.message.split(SEP).collect();
    if parts.len() == 3 && parts[0] == "rename" {
        format!(
            "Changed vcs type from {} to {} based on URL.",
            parts[1], parts[2]
        )
    } else {
        "Fix Vcs-* type mismatch.".to_string()
    }
}

declare_detector! {
    name: "vcs-field-mismatch",
    tags: ["vcs-field-mismatch"],
    after: ["vcs-broken-uri"],
    before: ["vcs-field-not-canonical"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Source",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Vcs-*",
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
    fn test_simple() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: lintian-brush\nVcs-Bzr: https://salsa.debian.org/jelmer/dulwich.git\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Changed vcs type from Bzr to Git based on URL."
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: lintian-brush\nVcs-Git: https://salsa.debian.org/jelmer/dulwich.git\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        );
    }

    #[test]
    fn test_no_op() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: lintian-brush\nVcs-Git: https://salsa.debian.org/jelmer/lintian-brush.git\nHomepage: https://www.jelmer.uk/lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
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
