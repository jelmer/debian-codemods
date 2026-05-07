use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use std::path::{Path, PathBuf};
use url::Url;

fn convert_certainty(upstream_certainty: upstream_ontologist::Certainty) -> Certainty {
    match upstream_certainty {
        upstream_ontologist::Certainty::Certain => Certainty::Certain,
        upstream_ontologist::Certainty::Confident => Certainty::Confident,
        upstream_ontologist::Certainty::Likely => Certainty::Likely,
        upstream_ontologist::Certainty::Possible => Certainty::Possible,
    }
}

fn guess_homepage(
    base_path: &Path,
    preferences: &FixerPreferences,
) -> Option<(String, upstream_ontologist::Certainty)> {
    let rt = tokio::runtime::Runtime::new().ok()?;

    let trust_package = if preferences.trust_package.unwrap_or(false) {
        Some(true)
    } else {
        None
    };
    let net_access = preferences.net_access;

    rt.block_on(async {
        let metadata = upstream_ontologist::guess_upstream_metadata(
            base_path,
            trust_package,
            net_access,
            None,
            None,
        )
        .await
        .ok()?;
        let homepage_datum = metadata.get("Homepage")?;
        if let Some(ref origin) = homepage_datum.origin {
            let origin_str = origin.to_string();
            if origin_str == "./debian/control" || origin_str == "debian/control" {
                return None;
            }
        }
        if let upstream_ontologist::UpstreamDatum::Homepage(url) = &homepage_datum.datum {
            let certainty = homepage_datum
                .certainty
                .unwrap_or(upstream_ontologist::Certainty::Possible);
            return Some((url.clone(), certainty));
        }
        None
    })
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let source_para = source.as_deb822();

    let existing_homepage = source_para.get("Homepage");
    let (issue, message_kind) = match existing_homepage.as_deref() {
        None => (
            LintianIssue::source_with_info("no-homepage-field", vec![String::new()]),
            "fill",
        ),
        Some(homepage) => {
            let Ok(url) = Url::parse(homepage) else {
                return Ok(Vec::new());
            };
            match url.host_str() {
                Some("pypi.org") => (
                    LintianIssue::source_with_info("pypi-homepage", vec![homepage.to_string()]),
                    "pypi",
                ),
                Some("rubygems.org") => (
                    LintianIssue::source_with_info("rubygem-homepage", vec![homepage.to_string()]),
                    "rubygem",
                ),
                _ => return Ok(Vec::new()),
            }
        }
    };

    let description = match message_kind {
        "pypi" => "Homepage field points at pypi.org.".to_string(),
        "rubygem" => "Homepage field points at rubygems.org.".to_string(),
        _ => "Homepage field is missing.".to_string(),
    };
    let label = match message_kind {
        "pypi" => "Avoid pypi.org in Homepage field.".to_string(),
        "rubygem" => "Avoid rubygems.org in Homepage field.".to_string(),
        _ => "Fill in Homepage field.".to_string(),
    };
    let homepage_guess = ws
        .base_path()
        .and_then(|base_path| guess_homepage(base_path, preferences));
    let Some((homepage_url, upstream_certainty)) = homepage_guess else {
        return Ok(vec![Diagnostic::with_actions(
            issue,
            description,
            label,
            vec![],
        )
        .with_certainty(Certainty::Possible)]);
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        description,
        label,
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Homepage".into(),
            value: homepage_url,
        })],
    )
    .with_certainty(convert_certainty(upstream_certainty))])
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let Some((first, _)) = fixed.first() else {
        return "Fill in Homepage field.".to_string();
    };
    let Some(plan) = first.plans.first() else {
        return "Fill in Homepage field.".to_string();
    };
    plan.label.clone()
}

declare_detector! {
    name: "no-homepage-field",
    tags: ["no-homepage-field", "pypi-homepage", "rubygem-homepage"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Homepage",
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
    fn test_homepage_already_exists() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nHomepage: https://example.com\nMaintainer: Test User <test@example.com>\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
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
    fn test_no_homepage_field_no_net() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nMaintainer: Test User <test@example.com>\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
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
