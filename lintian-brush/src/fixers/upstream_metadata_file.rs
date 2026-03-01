use crate::upstream_metadata::DEP12_FIELD_ORDER;
use crate::{FixerError, FixerResult, LintianIssue};
use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;
use tracing::debug;
use upstream_ontologist::vcs::convert_cvs_list_to_str;
use upstream_ontologist::{
    check_upstream_metadata, extend_upstream_metadata, guess_upstream_metadata_items,
    update_from_guesses, UpstreamMetadata,
};

fn upstream_metadata_sort_key(field_name: &str) -> usize {
    // Return the index in DEP12_FIELD_ORDER, or a large value for unknown fields
    DEP12_FIELD_ORDER
        .iter()
        .position(|&f| f == field_name)
        .unwrap_or(usize::MAX)
}

fn is_valid_dep12_field(field_name: &str) -> bool {
    DEP12_FIELD_ORDER.contains(&field_name)
}

pub fn run(
    base_path: &Path,
    current_version: &debversion::Version,
    preferences: &crate::FixerPreferences,
) -> Result<FixerResult, FixerError> {
    // Skip native packages
    if is_native_package(current_version)? {
        return Err(FixerError::NoChanges);
    }

    let metadata_path = base_path.join("debian/upstream/metadata");
    let file_existed = metadata_path.exists();

    // Check if the missing file issue should be fixed (respects lintian overrides)
    // We need to do this check BEFORE opening the YamlUpdater because opening it
    // will cause the Drop implementation to create the file on cleanup
    let missing_file_issue =
        LintianIssue::source_with_info("upstream-metadata-file-is-missing", vec![]);

    if !file_existed && !missing_file_issue.should_fix(base_path) {
        return Err(FixerError::NoChanges);
    }

    // Load or create the YAML document
    let doc = if metadata_path.exists() {
        yaml_edit::Document::from_file(&metadata_path)
            .map_err(|e| FixerError::Other(e.to_string()))?
    } else {
        // Create a new document initialized as a mapping
        let new_mapping = yaml_edit::Mapping::new();
        yaml_edit::Document::from_mapping(new_mapping)
    };

    // Get the mapping from the document
    let mapping = doc
        .as_mapping()
        .ok_or_else(|| FixerError::Other("Document is not a mapping".to_string()))?;

    // Capture original keys for tag checking later
    let original_keys: HashSet<String> = mapping
        .keys()
        .filter_map(|node| match node {
            yaml_edit::YamlNode::Scalar(scalar) => Some(scalar.as_string()),
            _ => None,
        })
        .collect();

    // Convert repository list to string if needed (for CVS repositories)
    let mut repository_converted = false;
    if let Some(repo_value) = mapping.get("Repository") {
        if let Some(sequence) = repo_value.as_sequence() {
            // Extract strings from the list
            let url_strings: Vec<String> = sequence
                .values()
                .filter_map(|node| {
                    if let yaml_edit::YamlNode::Scalar(scalar) = node {
                        Some(scalar.as_string())
                    } else {
                        None
                    }
                })
                .collect();

            // Convert to &str slice for the function
            let url_refs: Vec<&str> = url_strings.iter().map(|s: &String| s.as_str()).collect();

            // Try to convert using upstream-ontologist
            if let Some(converted) = convert_cvs_list_to_str(&url_refs) {
                debug!(
                    "Converting Repository from list to string: {:?} -> {}",
                    url_strings, converted
                );
                mapping.set("Repository", converted);
                repository_converted = true;
                // Note: Don't add to changed_fields here yet, we'll add it later if it's different
            }
        }
    }

    // Create async runtime for upstream-ontologist calls
    let _runtime = tokio::runtime::Runtime::new().map_err(|e| FixerError::Other(e.to_string()))?;

    // Get settings from preferences
    let trust_package = preferences.trust_package.unwrap_or(true);
    let net_access = preferences.net_access.unwrap_or(false);

    // Initialize tracing for debugging
    let _ = tracing_subscriber::fmt::try_init();

    debug!(
        "Upstream metadata fixer starting with trust_package={}, net_access={}",
        trust_package, net_access
    );
    // Convert minimum_certainty from our Certainty to upstream_ontologist::Certainty
    let minimum_certainty = preferences.minimum_certainty.as_ref().map(|c| match c {
        crate::Certainty::Certain => upstream_ontologist::Certainty::Certain,
        crate::Certainty::Confident => upstream_ontologist::Certainty::Confident,
        crate::Certainty::Likely => upstream_ontologist::Certainty::Likely,
        crate::Certainty::Possible => upstream_ontologist::Certainty::Possible,
    });
    let consult_external_directory = true;

    // Create initial upstream metadata from existing YAML (like Python version does)
    let mut upstream_metadata = UpstreamMetadata::new();

    // Load existing YAML data into upstream metadata with "certain" certainty
    // This mirrors the Python from_dict implementation
    let keys: Vec<String> = mapping
        .keys()
        .filter_map(|node| match node {
            yaml_edit::YamlNode::Scalar(scalar) => Some(scalar.as_string()),
            _ => None,
        })
        .collect();
    for key_str in &keys {
        if let Some(value) = mapping.get(key_str.as_str()) {
            let value_str = value.as_scalar().map(|scalar| scalar.value().to_string());
            if let Some(value_str) = value_str {
                // Create the appropriate UpstreamDatum variant based on field name
                // Based on the Python bindings in upstream-ontologist-py
                let datum = match key_str.as_str() {
                    "Name" => Some(upstream_ontologist::UpstreamDatum::Name(value_str)),
                    "Version" => Some(upstream_ontologist::UpstreamDatum::Version(value_str)),
                    "Summary" => Some(upstream_ontologist::UpstreamDatum::Summary(value_str)),
                    "Description" => {
                        Some(upstream_ontologist::UpstreamDatum::Description(value_str))
                    }
                    "Homepage" => Some(upstream_ontologist::UpstreamDatum::Homepage(value_str)),
                    "Repository" => Some(upstream_ontologist::UpstreamDatum::Repository(value_str)),
                    "Repository-Browse" => Some(
                        upstream_ontologist::UpstreamDatum::RepositoryBrowse(value_str),
                    ),
                    "License" => Some(upstream_ontologist::UpstreamDatum::License(value_str)),
                    "Bug-Database" => {
                        Some(upstream_ontologist::UpstreamDatum::BugDatabase(value_str))
                    }
                    "Bug-Submit" => Some(upstream_ontologist::UpstreamDatum::BugSubmit(value_str)),
                    "Contact" => Some(upstream_ontologist::UpstreamDatum::Contact(value_str)),
                    "Cargo-Crate" => {
                        Some(upstream_ontologist::UpstreamDatum::CargoCrate(value_str))
                    }
                    "Security-MD" => {
                        Some(upstream_ontologist::UpstreamDatum::SecurityMD(value_str))
                    }
                    "Security-Contact" => Some(
                        upstream_ontologist::UpstreamDatum::SecurityContact(value_str),
                    ),
                    "Documentation" => {
                        Some(upstream_ontologist::UpstreamDatum::Documentation(value_str))
                    }
                    "Go-Import-Path" => {
                        Some(upstream_ontologist::UpstreamDatum::GoImportPath(value_str))
                    }
                    "Download" => Some(upstream_ontologist::UpstreamDatum::Download(value_str)),
                    "Wiki" => Some(upstream_ontologist::UpstreamDatum::Wiki(value_str)),
                    "MailingList" => {
                        Some(upstream_ontologist::UpstreamDatum::MailingList(value_str))
                    }
                    "SourceForge-Project" => Some(
                        upstream_ontologist::UpstreamDatum::SourceForgeProject(value_str),
                    ),
                    "Archive" => Some(upstream_ontologist::UpstreamDatum::Archive(value_str)),
                    "Demo" => Some(upstream_ontologist::UpstreamDatum::Demo(value_str)),
                    "Pecl-Package" => {
                        Some(upstream_ontologist::UpstreamDatum::PeclPackage(value_str))
                    }
                    "Haskell-Package" => Some(upstream_ontologist::UpstreamDatum::HaskellPackage(
                        value_str,
                    )),
                    "Funding" => Some(upstream_ontologist::UpstreamDatum::Funding(value_str)),
                    "Changelog" => Some(upstream_ontologist::UpstreamDatum::Changelog(value_str)),
                    "Debian-ITP" => value_str
                        .parse()
                        .ok()
                        .map(upstream_ontologist::UpstreamDatum::DebianITP),
                    "Screenshots" => Some(upstream_ontologist::UpstreamDatum::Screenshots(vec![
                        value_str,
                    ])),
                    "Cite-As" => Some(upstream_ontologist::UpstreamDatum::CiteAs(value_str)),
                    "Registry" => {
                        // Registry expects Vec<(String, String)>, parse as simple name:url pair for now
                        // TODO: Parse properly from YAML list/mapping
                        None
                    }
                    "Donation" => Some(upstream_ontologist::UpstreamDatum::Donation(value_str)),
                    "Webservice" => Some(upstream_ontologist::UpstreamDatum::Webservice(value_str)),
                    "FAQ" => Some(upstream_ontologist::UpstreamDatum::FAQ(value_str)),
                    // These fields don't exist in upstream_ontologist, skip them
                    "Registration" => None,
                    "Gallery" => None,
                    "CPE" => None,
                    "ASCL-Id" => None,
                    "Other-References" => None,
                    "Reference" => None,
                    _ => None, // Skip unknown fields
                };

                if let Some(datum) = datum {
                    let datum_with_metadata = upstream_ontologist::UpstreamDatumWithMetadata {
                        datum,
                        certainty: Some(upstream_ontologist::Certainty::Certain),
                        origin: None,
                    };
                    upstream_metadata.insert(datum_with_metadata);
                }
            }
        }
    }

    let runtime = tokio::runtime::Runtime::new().map_err(|e| FixerError::Other(e.to_string()))?;

    // Downgrade minimum certainty for initial operations, since check_upstream_metadata can
    // upgrade it to "certain" later (matches Python logic)
    let initial_minimum_certainty =
        if net_access && minimum_certainty == Some(upstream_ontologist::Certainty::Certain) {
            Some(upstream_ontologist::Certainty::Likely)
        } else {
            minimum_certainty
        };

    debug!(
        "minimum_certainty={:?}, initial_minimum_certainty={:?}",
        minimum_certainty, initial_minimum_certainty
    );

    // Step 1: Do some guessing based on what's in the package (like Python version)
    debug!(
        "Calling guess_upstream_metadata_items with path={:?}, trust_package={}, certainty={:?}",
        base_path, trust_package, initial_minimum_certainty
    );
    let guessed_items_stream =
        guess_upstream_metadata_items(base_path, Some(trust_package), initial_minimum_certainty);

    // Collect the stream and filter bad guesses
    let guessed_items: Vec<_> = runtime.block_on(async {
        use futures::StreamExt;
        guessed_items_stream.collect().await
    });

    debug!(
        "guess_upstream_metadata_items returned {} items",
        guessed_items.len()
    );
    for item in &guessed_items {
        match item {
            Ok(datum_with_metadata) => {
                debug!(
                    "Guessed item: {} = {:?} (certainty: {:?})",
                    datum_with_metadata.datum.field(),
                    datum_with_metadata.datum.as_str(),
                    datum_with_metadata.certainty
                );
            }
            Err(e) => debug!("Failed item: {:?}", e),
        }
    }

    let filtered_items: Vec<_> = guessed_items
        .into_iter()
        .filter_map(|item| item.ok())
        .filter(|item| !item.datum.known_bad_guess())
        .collect();

    debug!(
        "After filtering bad guesses: {} items remain",
        filtered_items.len()
    );
    for item in &filtered_items {
        debug!(
            "Filtered item: {} = {:?} (certainty: {:?})",
            item.datum.field(),
            item.datum.as_str(),
            item.certainty
        );
    }

    update_from_guesses(upstream_metadata.mut_items(), filtered_items.into_iter());

    // Step 2: Then extend that by contacting e.g. SourceForge (like Python version)
    let _ = runtime.block_on(async {
        extend_upstream_metadata(
            &mut upstream_metadata,
            base_path,
            initial_minimum_certainty,
            Some(net_access),
            Some(consult_external_directory),
        )
        .await
    });

    // Step 3: If net access, verify that online resources actually exist (like Python version)
    if net_access {
        // Get upstream version from current_version
        let upstream_version = get_current_version(preferences).map(|v| {
            let upstream = v.upstream_version;
            // Remove ~ or + suffixes like Python does
            if let Some(pos) = upstream.find('~') {
                upstream[..pos].to_string()
            } else if let Some(pos) = upstream.find('+') {
                upstream[..pos].to_string()
            } else {
                upstream
            }
        });
        runtime.block_on(async {
            check_upstream_metadata(&mut upstream_metadata, upstream_version.as_deref()).await
        });
    }

    let mut guessed_metadata = upstream_metadata;

    // Call fix_upstream_metadata to canonicalize URLs (adds .git suffix, etc.)
    // This matches what the Python script does
    runtime.block_on(async {
        upstream_ontologist::fix_upstream_metadata(&mut guessed_metadata).await;
    });

    debug!("After fix_upstream_metadata, guessed_metadata has entries");
    for item in guessed_metadata.iter() {
        if item.datum.field() == "Repository" {
            debug!("Repository value after fix: {:?}", item.datum.as_str());
        }
    }

    // Custom sort using upstream_metadata_sort_key logic
    guessed_metadata.mut_items().sort_by(|a, b| {
        let key_a = upstream_metadata_sort_key(a.datum.field());
        let key_b = upstream_metadata_sort_key(b.datum.field());
        key_a.cmp(&key_b)
    });

    // Check if Name/Contact are in copyright file (if they exist in metadata)
    let mut external_present_fields = HashSet::new();
    external_present_fields.insert("Homepage"); // Homepage is in debian/control

    // If we have Name or Contact in the guessed metadata, check if they're in copyright
    let has_name_or_contact = guessed_metadata
        .iter()
        .any(|d| matches!(d.datum.field(), "Name" | "Contact"));

    if has_name_or_contact {
        // Check if debian/copyright is machine-readable and has Name/Contact
        let copyright_path = base_path.join("debian/copyright");
        if copyright_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&copyright_path) {
                if let Ok(copyright) = debian_copyright::lossless::Copyright::from_str(&content) {
                    // Check for upstream name and contact in the header
                    if let Some(header) = copyright.header() {
                        if header.upstream_name().is_some() {
                            external_present_fields.insert("Name");
                        }
                        if header.upstream_contact().is_some() {
                            external_present_fields.insert("Contact");
                        }
                    }
                }
            }
        }
    }

    // Filter metadata like Python does AFTER calculating external_present_fields:
    // 1. Remove non-DEP12 fields
    // 2. Remove fields in external files
    // 3. Remove fields below minimum certainty
    guessed_metadata.mut_items().retain(|item| {
        let field_name = item.datum.field();
        // Only include DEP12 fields not in external files
        let is_dep12_field = is_valid_dep12_field(field_name);
        let not_external = !external_present_fields.contains(field_name);

        // Check minimum certainty requirement (matches Python meets_minimum_certainty)
        let meets_min_certainty = if let Some(item_certainty) = item.certainty {
            if let Some(min_certainty) = minimum_certainty {
                // Item certainty must be at least the minimum certainty
                // Certainty enum: Possible < Likely < Confident < Certain
                // So we want item_certainty >= min_certainty
                item_certainty >= min_certainty
            } else {
                true
            }
        } else {
            true // Python ignores unknown certainty
        };

        is_dep12_field && not_external && meets_min_certainty
    });

    // Calculate certainty from filtered metadata (like Python does)
    let filtered_certainties: Vec<_> = guessed_metadata
        .iter()
        .filter_map(|item| item.certainty)
        .collect();
    debug!("Post-filter certainties: {:?}", filtered_certainties);

    let mut changed_fields: Vec<(String, Option<String>)> = Vec::new(); // (field_name, origin)
    let mut certainties: Vec<upstream_ontologist::Certainty> = Vec::new();

    // Collect fields to update
    let mut fields_to_update: Vec<(&str, String)> = Vec::new();

    // Merge guessed metadata with existing metadata
    for datum_with_metadata in guessed_metadata.iter() {
        let field_name = datum_with_metadata.datum.field();
        let value = datum_with_metadata.datum.as_str();
        let origin = datum_with_metadata.origin.as_ref().map(|o| o.to_string());

        // Only keep fields that are valid DEP12 fields
        // Skip Homepage as it's in debian/control
        // Skip fields that aren't DEP12 compliant
        if !is_valid_dep12_field(field_name) || external_present_fields.contains(field_name) {
            continue;
        }

        // Check if the field doesn't exist OR if the value is different from what we have
        let should_update = if !mapping.keys().any(|node| match node {
            yaml_edit::YamlNode::Scalar(scalar) => scalar.as_string() == field_name,
            _ => false,
        }) {
            true
        } else if let Some(new_value) = value.map(|s| s.to_string()) {
            // Get existing value
            if let Some(existing_value) = mapping.get(field_name) {
                if let Some(scalar) = existing_value.as_scalar() {
                    scalar.value() != new_value
                } else {
                    true // Different type, need to update
                }
            } else {
                true
            }
        } else {
            false
        };

        if should_update {
            if let Some(v) = value {
                debug!("Will update field {} with value: {:?}", field_name, v);
                fields_to_update.push((field_name, v.to_string()));
                changed_fields.push((field_name.to_string(), origin));
                if let Some(c) = datum_with_metadata.certainty {
                    certainties.push(c);
                }
            }
        }
    }

    // Apply updates using set_with_field_order for proper DEP-12 field ordering
    if !fields_to_update.is_empty() {
        for (k, v) in fields_to_update {
            doc.set_with_field_order(k, v, DEP12_FIELD_ORDER);
        }
    }

    // If repository was converted, add it to changed_fields
    if repository_converted {
        changed_fields.push(("Repository".to_string(), None));
        // Use "certain" certainty for repository conversions
        certainties.push(upstream_ontologist::Certainty::Certain);
    }

    debug!(
        "Changed fields: {:?}, repository_converted: {}",
        changed_fields, repository_converted
    );

    if changed_fields.is_empty() && !repository_converted {
        debug!("No changes detected, returning NoChanges");
        return Err(FixerError::NoChanges);
    }

    // Skip if only non-substantive fields would be added and no repository conversion
    let substantive_fields = changed_fields
        .iter()
        .filter(|(field, _)| !matches!(field.as_str(), "Name" | "Contact"))
        .count();

    if substantive_fields == 0 && !repository_converted {
        return Err(FixerError::NoChanges);
    }

    // Create the debian/upstream directory if it doesn't exist and we have content
    if let Some(parent) = metadata_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| FixerError::Other(e.to_string()))?;
        }
    }

    // Save the document
    doc.to_file(&metadata_path)
        .map_err(|e| FixerError::Other(e.to_string()))?;

    // Determine which lintian issues were fixed
    // Following the Python logic, only report tags as fixed if fields were actually missing before
    let mut fixed_issues = Vec::new();

    // Helper to check if all fields in a set existed in original metadata
    let all_fields_existed =
        |fields: &[&str]| -> bool { fields.iter().all(|field| original_keys.contains(*field)) };

    // Helper to check if all fields exist now (either originally or newly added)
    let all_fields_exist_now = |fields: &[&str]| -> bool {
        fields.iter().all(|field| {
            original_keys.contains(*field) || changed_fields.iter().any(|(f, _)| f == field)
        })
    };

    // Check Repository fields (Repository, Repository-Browse)
    let repository_fields = ["Repository", "Repository-Browse"];
    if !all_fields_existed(&repository_fields) && all_fields_exist_now(&repository_fields) {
        fixed_issues.push(LintianIssue::source_with_info(
            "upstream-metadata-missing-repository",
            vec!["[debian/upstream/metadata]".to_string()],
        ));
    }

    // Check bug tracking fields (Bug-Database, Bug-Submit)
    let bug_fields = ["Bug-Database", "Bug-Submit"];
    if !all_fields_existed(&bug_fields) && all_fields_exist_now(&bug_fields) {
        fixed_issues.push(LintianIssue::source_with_info(
            "upstream-metadata-missing-bug-tracking",
            vec!["[debian/upstream/metadata]".to_string()],
        ));
    }

    // Check if we created the file (add this at the end to match expected order)
    if !file_existed && missing_file_issue.should_fix(base_path) {
        fixed_issues.push(missing_file_issue);
    }

    // Sort changed fields alphabetically for consistent description message
    changed_fields.sort_by(|a, b| a.0.cmp(&b.0));

    // Format fields with origin information where applicable
    // Only add "(from origin)" for ./configure origin, matching Python implementation
    let formatted_fields: Vec<String> = changed_fields
        .iter()
        .map(|(field, origin)| {
            if let Some(o) = origin {
                if o == "./configure" {
                    format!("{} (from {})", field, o)
                } else {
                    field.clone()
                }
            } else {
                field.clone()
            }
        })
        .collect();

    // Build description - if repository was converted, it's already in changed_fields
    let description = format!(
        "Set upstream metadata fields: {}.",
        formatted_fields.join(", ")
    );

    // Calculate final certainty from the certainties of fields that were actually changed
    debug!("Final certainties for changed fields: {:?}", certainties);
    let achieved_certainty = if certainties.is_empty() {
        crate::Certainty::Likely
    } else {
        // Find the minimum certainty (most conservative)
        let min_certainty = certainties.into_iter().min().unwrap();
        debug!("Minimum certainty for output: {:?}", min_certainty);
        match min_certainty {
            upstream_ontologist::Certainty::Certain => crate::Certainty::Certain,
            upstream_ontologist::Certainty::Confident => crate::Certainty::Confident,
            upstream_ontologist::Certainty::Likely => crate::Certainty::Likely,
            upstream_ontologist::Certainty::Possible => crate::Certainty::Possible,
        }
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .certainty(achieved_certainty)
        .build())
}

fn get_current_version(preferences: &crate::FixerPreferences) -> Option<debversion::Version> {
    // Check CURRENT_VERSION from preferences.extra_env
    if let Some(extra_env) = &preferences.extra_env {
        debug!("extra_env contains: {:?}", extra_env);
        if let Some(version_str) = extra_env.get("CURRENT_VERSION") {
            debug!("Found CURRENT_VERSION in extra_env: {}", version_str);
            if let Ok(version) = version_str.parse::<debversion::Version>() {
                debug!("Parsed version from extra_env: {:?}", version);
                return Some(version);
            }
        }
    }

    // Fallback: try environment variable
    if let Ok(version_str) = std::env::var("CURRENT_VERSION") {
        debug!("Found CURRENT_VERSION in env: {}", version_str);
        if let Ok(version) = version_str.parse::<debversion::Version>() {
            debug!("Parsed version from env: {:?}", version);
            return Some(version);
        }
    }

    debug!("No CURRENT_VERSION found");
    None
}

fn is_native_package(current_version: &debversion::Version) -> Result<bool, FixerError> {
    let is_native = current_version.debian_revision.is_none();
    debug!(
        "is_native_package: version={:?}, is_native={}",
        current_version, is_native
    );
    Ok(is_native)
}

declare_fixer! {
    name: "upstream-metadata-file",
    tags: [
        "upstream-metadata-file-is-missing",
        "upstream-metadata-missing-bug-tracking",
        "upstream-metadata-missing-repository"
    ],
    apply: |basedir, _package, version, preferences| {
        run(basedir, version, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_javascript_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create a simple package.json
        let package_json = r#"{
  "name": "test-package",
  "description": "A test package",
  "version": "1.0.0",
  "repository": {
    "type": "git",
    "url": "https://github.com/example/test-package.git"
  },
  "homepage": "https://github.com/example/test-package"
}"#;

        fs::write(base_path.join("package.json"), package_json).unwrap();

        // Create debian directory structure
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create a non-native changelog
        let changelog = "test-package (1.0.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- Test <test@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        fs::write(debian_dir.join("changelog"), changelog).unwrap();

        let preferences = crate::FixerPreferences::default();
        let version: debversion::Version = "1.0.0-1".parse().unwrap();
        let result = run(base_path, &version, &preferences).unwrap();
        assert!(result.description.contains("Set upstream metadata fields"));

        // Check that metadata file was created
        let metadata_path = base_path.join("debian/upstream/metadata");
        assert!(metadata_path.exists());

        let metadata_content = fs::read_to_string(&metadata_path).unwrap();
        assert!(metadata_content.contains("Name: test-package"));
        assert!(metadata_content.contains("Repository:"));
    }

    #[test]
    fn test_native_package_skipped() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create a native package changelog (no debian revision)
        let changelog = "test-package (1.0.0) unstable; urgency=medium\n\n  * Initial release.\n\n -- Test <test@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        fs::write(debian_dir.join("changelog"), changelog).unwrap();

        let preferences = crate::FixerPreferences::default();
        let version: debversion::Version = "1.0.0".parse().unwrap();
        let result = run(base_path, &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_package_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create a non-native changelog
        let changelog = "test-package (1.0.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- Test <test@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        fs::write(debian_dir.join("changelog"), changelog).unwrap();

        let preferences = crate::FixerPreferences::default();
        let version: debversion::Version = "1.0.0-1".parse().unwrap();
        let result = run(base_path, &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
