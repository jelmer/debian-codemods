use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{min_certainty, FixerError, FixerPreferences};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};
use upstream_ontologist::UpstreamDatum;

fn convert_certainty(upstream_certainty: upstream_ontologist::Certainty) -> crate::Certainty {
    match upstream_certainty {
        upstream_ontologist::Certainty::Certain => crate::Certainty::Certain,
        upstream_ontologist::Certainty::Confident => crate::Certainty::Confident,
        upstream_ontologist::Certainty::Likely => crate::Certainty::Likely,
        upstream_ontologist::Certainty::Possible => crate::Certainty::Possible,
    }
}

fn guess_upstream_metadata(
    base_path: &Path,
    preferences: &FixerPreferences,
) -> Option<upstream_ontologist::UpstreamMetadata> {
    use futures::StreamExt;

    let rt = tokio::runtime::Runtime::new().ok()?;
    let trust_package = if preferences.trust_package.unwrap_or(false) {
        Some(true)
    } else {
        None
    };

    rt.block_on(async {
        let stream =
            upstream_ontologist::guess_upstream_metadata_items(base_path, trust_package, None);
        let items: Vec<upstream_ontologist::UpstreamDatumWithMetadata> = stream
            .filter_map(|result| async move { result.ok() })
            .collect()
            .await;
        Some(items.into())
    })
}

pub fn detect(
    ws: &dyn Workspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let copyright = match ws.parsed_copyright() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };
    let Some(header) = copyright.header() else {
        return Ok(Vec::new());
    };

    if header.upstream_name().is_some() && header.upstream_contact().is_some() {
        return Ok(Vec::new());
    }

    // upstream-ontologist needs to walk the source tree, which only the
    // tree-mode host can provide. LSP-style hosts have to skip.
    let Some(base_path) = ws.base_path() else {
        return Ok(Vec::new());
    };

    let Some(mut upstream_metadata) = guess_upstream_metadata(base_path, preferences) else {
        return Ok(Vec::new());
    };

    // Fold in debian/upstream/metadata, marking anything found there as
    // Certain (it's authoritative for this package).
    if let Ok(yaml) = ws.parsed_upstream_metadata() {
        if let Some(doc) = yaml.document() {
            for key_str in &["Name", "Contact"] {
                let Some(node) = doc.get(*key_str) else {
                    continue;
                };
                let Some(value) = node.as_scalar() else {
                    continue;
                };
                let value_str = value.value();
                let datum = match *key_str {
                    "Name" => UpstreamDatum::Name(value_str),
                    "Contact" => UpstreamDatum::Contact(value_str),
                    _ => continue,
                };
                let should_replace = upstream_metadata
                    .get(key_str)
                    .map(|d| d.certainty != Some(upstream_ontologist::Certainty::Certain))
                    .unwrap_or(true);
                if !should_replace {
                    continue;
                }
                upstream_metadata.remove(key_str);
                upstream_metadata.insert(upstream_ontologist::UpstreamDatumWithMetadata {
                    datum,
                    certainty: Some(upstream_ontologist::Certainty::Certain),
                    origin: Some(upstream_ontologist::Origin::Other(
                        "debian/upstream/metadata".to_string(),
                    )),
                });
            }
        }
    }

    let mut actions: Vec<Action> = Vec::new();
    let mut fields: Vec<&'static str> = Vec::new();
    let mut certainties: Vec<upstream_ontologist::Certainty> = Vec::new();

    let needs_name = header.upstream_name().is_none();
    let needs_contact = header.upstream_contact().is_none();

    let mut take = |key: &str,
                    field: &'static str,
                    extract: fn(&UpstreamDatum) -> Option<String>|
     -> Option<()> {
        let datum = upstream_metadata.get(key)?;
        let cert = datum
            .certainty
            .unwrap_or(upstream_ontologist::Certainty::Possible);
        let value = extract(&datum.datum)?;
        if value.is_empty() {
            return None;
        }
        actions.push(Action::Deb822(Deb822Action::SetField {
            file: copyright_rel.clone(),
            paragraph: ParagraphSelector::CopyrightHeader,
            field: field.to_string(),
            value,
        }));
        fields.push(field);
        certainties.push(cert);
        Some(())
    };

    if needs_name {
        take("Name", "Upstream-Name", |d| match d {
            UpstreamDatum::Name(n) => Some(n.clone()),
            _ => None,
        });
    }
    if needs_contact {
        take("Contact", "Upstream-Contact", |d| match d {
            UpstreamDatum::Contact(c) => Some(c.clone()),
            _ => None,
        });
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let converted: Vec<crate::Certainty> =
        certainties.iter().map(|c| convert_certainty(*c)).collect();
    // The fields are demonstrably missing, so the diagnostic is Certain.
    // The values are guessed, so the plan is only as certain as its
    // least certain field.
    let plan_certainty = min_certainty(&converted).unwrap_or(crate::Certainty::Possible);

    let label = if fields.len() == 1 {
        format!("Set field {} in debian/copyright.", fields[0])
    } else {
        format!("Set fields {} in debian/copyright.", fields.join(", "))
    };
    let description = if fields.len() == 1 {
        format!("debian/copyright is missing field {}.", fields[0])
    } else {
        format!("debian/copyright is missing fields {}.", fields.join(", "))
    };

    Ok(vec![Diagnostic::untagged_with_plans(
        description,
        vec![ActionPlan {
            label,
            opinionated: false,
            certainty: Some(plan_certainty),
            actions,
        }],
    )
    .with_certainty(crate::Certainty::Certain)])
}

declare_detector! {
    name: "copyright-missing-upstream-info",
    tags: [],
    cost: crate::detector::DetectorCost::Network,
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(
        base: &Path,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test-package".into()),
                Some(v.clone()),
            );
            adapter.apply(&ws, preferences)
        }
    }

    #[test]
    fn test_both_fields_present() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test-package\nUpstream-Contact: Test User <test@example.com>\n\nFiles: *\nCopyright: 2024 Test User <test@example.com>\nLicense: GPL-3+\n\nLicense: GPL-3+\n This program is free software.\n",
        )
        .unwrap();
        let prefs = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_upstream_metadata_available() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: 2024 Test User <test@example.com>\nLicense: GPL-3+\n\nLicense: GPL-3+\n This program is free software.\n",
        )
        .unwrap();
        let prefs = FixerPreferences {
            net_access: Some(false),
            trust_package: Some(false),
            minimum_certainty: Some(crate::Certainty::Likely),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_not_machine_readable() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "This is not a machine-readable copyright file.\n",
        )
        .unwrap();
        let prefs = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_copyright_file_missing() {
        let tmp = TempDir::new().unwrap();
        let prefs = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }
}
