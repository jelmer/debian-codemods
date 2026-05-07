use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::collections::BTreeSet;
use std::path::PathBuf;

const SEP: char = '\t';

/// Find a secure VCS URL using upstream-ontologist's host knowledge.
async fn find_secure_vcs_url(
    url: &str,
    net_access: bool,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let parsed: debian_control::vcs::ParsedVcs = url.parse()?;
    let repo_url = match url::Url::parse(&parsed.repo_url) {
        Ok(u) => u,
        Err(_) => return Ok(None),
    };
    let secure_repo_url = upstream_ontologist::vcs::find_secure_repo_url(
        repo_url,
        parsed.branch.as_deref(),
        Some(net_access),
    )
    .await;
    match secure_repo_url {
        Some(secure_url) => {
            let result = debian_control::vcs::ParsedVcs {
                repo_url: secure_url.to_string(),
                branch: parsed.branch,
                subpath: parsed.subpath,
            };
            Ok(Some(result.to_string()))
        }
        None => Ok(None),
    }
}

const VCS_TYPES: &[&str] = &[
    "Git", "Browser", "Svn", "Bzr", "Hg", "Cvs", "Arch", "Darcs", "Mtn", "Svk",
];

pub fn detect(
    ws: &dyn FixerWorkspace,
    preferences: &FixerPreferences,
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
    let para = source.as_deb822();

    let net_access = preferences.net_access.unwrap_or(false);
    let Ok(rt) = tokio::runtime::Runtime::new() else {
        return Ok(Vec::new());
    };

    let mut diagnostics = Vec::new();
    let mut lp_seen = false;
    for vcs_type in VCS_TYPES {
        let field = format!("Vcs-{}", vcs_type);
        let Some(url) = para.get(&field) else {
            continue;
        };
        if url.starts_with("lp:") {
            lp_seen = true;
        }
        let Ok(Some(new_url)) = rt.block_on(find_secure_vcs_url(&url, net_access)) else {
            continue;
        };
        if new_url == url {
            continue;
        }

        let issue = LintianIssue::source_with_info(
            "vcs-field-uses-insecure-uri",
            vec![format!("{} {}", field, url)],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!(
                "field{}{}{}{}",
                SEP,
                field,
                SEP,
                if lp_seen { "1" } else { "0" }
            ),
            vec![Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::ByKey {
                    field: "Source".into(),
                    value: source_name.clone(),
                },
                field: field.clone(),
                value: new_url,
            })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let mut fields: BTreeSet<String> = BTreeSet::new();
    let mut lp_note = false;
    for d in fixed {
        let parts: Vec<&str> = d.message.split(SEP).collect();
        if parts.len() == 3 && parts[0] == "field" {
            fields.insert(parts[1].to_string());
            if parts[2] == "1" {
                lp_note = true;
            }
        }
    }
    let mut out = if fields.len() == 1 {
        format!(
            "Use secure URI in Vcs control header {}.",
            fields.iter().next().unwrap()
        )
    } else {
        format!(
            "Use secure URI in Vcs control headers: {}.",
            fields.iter().cloned().collect::<Vec<_>>().join(", ")
        )
    };
    if lp_note {
        out.push('\n');
        out.push('\n');
        out.push_str(
            "The lp: prefix gets expanded to http://code.launchpad.net/ for users that are not logged in on some versions of Bazaar.",
        );
    }
    out
}

declare_detector! {
    name: "vcs-field-uses-insecure-uri",
    tags: ["vcs-field-uses-insecure-uri"],
    after: ["vcs-field-not-canonical"],
    before: ["vcs-field-uses-not-recommended-uri-format"],
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
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(
        base: &Path,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test-package", &version, preferences)
    }

    #[test]
    fn test_http_to_https_no_net_access() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test-package\nVcs-Git: http://github.com/jelmer/test\n\nPackage: test-package\nDescription: Test\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        let result = run_apply(tmp.path(), &preferences).unwrap();
        assert_eq!(
            result.description,
            "Use secure URI in Vcs control header Vcs-Git."
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-package\nVcs-Git: https://github.com/jelmer/test\n\nPackage: test-package\nDescription: Test\n Test test\n",
        );
    }

    #[test]
    fn test_already_https() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nVcs-Git: https://github.com/jelmer/test\n\nPackage: test-package\nDescription: Test\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &preferences),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_multiple_vcs_fields() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test-package\nVcs-Git: http://github.com/jelmer/test\nVcs-Browser: http://github.com/jelmer/test\n\nPackage: test-package\nDescription: Test\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        let result = run_apply(tmp.path(), &preferences).unwrap();
        assert_eq!(
            result.description,
            "Use secure URI in Vcs control headers: Vcs-Browser, Vcs-Git."
        );
        let content = fs::read_to_string(&control).unwrap();
        assert!(content.contains("Vcs-Git: https://github.com/jelmer/test"));
        assert!(content.contains("Vcs-Browser: https://github.com/jelmer/test"));
    }

    #[test]
    fn test_no_vcs_fields() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\n\nPackage: test-package\nDescription: Test\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &preferences),
            Err(FixerError::NoChanges)
        ));
    }
}
