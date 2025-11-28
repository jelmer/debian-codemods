use crate::{certainty_sufficient, min_certainty, FixerError, FixerPreferences, FixerResult};
use debian_copyright::lossless::Copyright;
use std::path::Path;
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

    // Create a tokio runtime to call the async function
    let rt = tokio::runtime::Runtime::new().ok()?;

    let trust_package = if preferences.trust_package.unwrap_or(false) {
        Some(true)
    } else {
        None
    };

    rt.block_on(async {
        // Use guess_upstream_metadata_items (like the Python version) which doesn't
        // do extra mapping like Maintainer -> Contact
        let stream = upstream_ontologist::guess_upstream_metadata_items(
            base_path,
            trust_package,
            None, // minimum_certainty (we'll filter later)
        );

        let items: Vec<upstream_ontologist::UpstreamDatumWithMetadata> = stream
            .filter_map(|result| async move { result.ok() })
            .collect()
            .await;

        Some(items.into())
    })
}

pub fn run(
    base_path: &Path,
    _package_name: &str,
    preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");
    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = std::fs::read_to_string(&copyright_path)?;
    let copyright: Copyright = content
        .parse()
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/copyright: {:?}", e)))?;

    let header = copyright
        .header()
        .ok_or_else(|| FixerError::Other("No header paragraph in debian/copyright".to_string()))?;

    // Check if both fields are already present
    if header.upstream_name().is_some() && header.upstream_contact().is_some() {
        return Err(FixerError::NoChanges);
    }

    // Get upstream metadata
    let mut upstream_metadata =
        guess_upstream_metadata(base_path, preferences).ok_or(FixerError::NoChanges)?;

    // Also check debian/upstream/metadata if it exists
    // These entries should override guessed metadata (unless already "Certain")
    let upstream_metadata_path = base_path.join("debian/upstream/metadata");
    if upstream_metadata_path.exists() {
        let upstream_metadata_content = std::fs::read_to_string(&upstream_metadata_path)?;
        if let Ok(yaml_value) =
            serde_yaml::from_str::<serde_yaml::Value>(&upstream_metadata_content)
        {
            if let Some(mapping) = yaml_value.as_mapping() {
                for (key, value) in mapping {
                    if let (Some(key_str), Some(value_str)) = (key.as_str(), value.as_str()) {
                        // Replace if existing entry is not "Certain"
                        let should_replace = if let Some(existing) = upstream_metadata.get(key_str)
                        {
                            existing.certainty != Some(upstream_ontologist::Certainty::Certain)
                        } else {
                            true
                        };

                        if should_replace {
                            let datum = match key_str {
                                "Name" => UpstreamDatum::Name(value_str.to_string()),
                                "Contact" => UpstreamDatum::Contact(value_str.to_string()),
                                _ => continue,
                            };

                            // Remove existing entry if present
                            upstream_metadata.remove(key_str);

                            // Insert new entry with Certain certainty
                            upstream_metadata.insert(
                                upstream_ontologist::UpstreamDatumWithMetadata {
                                    datum,
                                    certainty: Some(upstream_ontologist::Certainty::Certain),
                                    origin: Some(upstream_ontologist::Origin::Other(
                                        "debian/upstream/metadata".to_string(),
                                    )),
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    let mut fields = Vec::new();
    let mut certainties = Vec::new();
    let mut made_changes = false;

    let header = copyright
        .header()
        .ok_or_else(|| FixerError::Other("No header paragraph in debian/copyright".to_string()))?;

    // Check what we need to set
    let needs_upstream_name = header.upstream_name().is_none();
    let needs_upstream_contact = header.upstream_contact().is_none();

    // Set Upstream-Name if missing
    if needs_upstream_name {
        if let Some(name_datum) = upstream_metadata.get("Name") {
            let datum_certainty = name_datum
                .certainty
                .unwrap_or(upstream_ontologist::Certainty::Possible);

            // Check if certainty is sufficient
            if !certainty_sufficient(
                convert_certainty(datum_certainty),
                preferences.minimum_certainty,
            ) {
                // Skip this datum due to insufficient certainty
            } else if let UpstreamDatum::Name(name) = &name_datum.datum {
                if !name.is_empty() {
                    copyright.header().unwrap().set_upstream_name(name);
                    fields.push("Upstream-Name");
                    certainties.push(datum_certainty);
                    made_changes = true;
                }
            }
        }
    }

    // Set Upstream-Contact if missing
    if needs_upstream_contact {
        if let Some(contact_datum) = upstream_metadata.get("Contact") {
            let datum_certainty = contact_datum
                .certainty
                .unwrap_or(upstream_ontologist::Certainty::Possible);

            // Check if certainty is sufficient
            if !certainty_sufficient(
                convert_certainty(datum_certainty),
                preferences.minimum_certainty,
            ) {
                // Skip this datum due to insufficient certainty
            } else if let UpstreamDatum::Contact(contact) = &contact_datum.datum {
                if !contact.is_empty() {
                    copyright.header().unwrap().set_upstream_contact(contact);
                    fields.push("Upstream-Contact");
                    certainties.push(datum_certainty);
                    made_changes = true;
                }
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    // Write back the copyright file
    std::fs::write(&copyright_path, copyright.to_string())?;

    let converted_certainties: Vec<crate::Certainty> =
        certainties.iter().map(|c| convert_certainty(*c)).collect();
    let certainty = min_certainty(&converted_certainties).unwrap_or(crate::Certainty::Possible);

    let description = if fields.len() == 1 {
        format!("Set field {} in debian/copyright.", fields[0])
    } else {
        format!("Set fields {} in debian/copyright.", fields.join(", "))
    };

    Ok(FixerResult::builder(description)
        .certainty(certainty)
        .build())
}

use crate::declare_fixer;

declare_fixer! {
    name: "copyright-missing-upstream-info",
    tags: [],
    apply: |basedir, package, _version, preferences| {
        run(basedir, package, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_both_fields_present() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: test-package
Upstream-Contact: Test User <test@example.com>

Files: *
Copyright: 2024 Test User <test@example.com>
License: GPL-3+

License: GPL-3+
 This program is free software.
"#;
        fs::write(debian_dir.join("copyright"), copyright_content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, "test-package", &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_upstream_metadata_available() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test User <test@example.com>
License: GPL-3+

License: GPL-3+
 This program is free software.
"#;
        fs::write(debian_dir.join("copyright"), copyright_content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            trust_package: Some(false),
            minimum_certainty: Some(crate::Certainty::Likely), // Require at least "likely" certainty
            ..Default::default()
        };

        let result = run(base_path, "test-package", &preferences);
        // Should not make changes when only low-certainty metadata available
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_copyright_file_missing() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, "test-package", &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
