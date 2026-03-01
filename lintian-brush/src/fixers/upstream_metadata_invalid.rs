use crate::{FixerError, FixerPreferences, FixerResult};
use std::collections::HashMap;
use std::path::Path;
use tracing::debug;

const SEQUENCE_FIELDS: &[&str] = &["Reference", "Screenshots"];

/// Fix duplicate keys by merging them
/// For sequence fields, merge values into a list
/// For other fields, keep the first value
fn fix_duplicate_keys(base_path: &Path) -> Result<Option<Vec<String>>, FixerError> {
    let metadata_path = base_path.join("debian/upstream/metadata");

    if !metadata_path.exists() {
        return Ok(None);
    }

    let doc = yaml_edit::Document::from_file(&metadata_path)
        .map_err(|e| FixerError::Other(format!("Failed to open YAML: {}", e)))?;

    let Some(mapping) = doc.as_mapping() else {
        return Ok(None);
    };

    // Collect all keys with their actual YamlNode values to detect duplicates
    let mut key_values: HashMap<String, Vec<yaml_edit::YamlNode>> = HashMap::new();

    for (key, value) in mapping.iter() {
        if let yaml_edit::YamlNode::Scalar(key_scalar) = key {
            let key_str = key_scalar.as_string();
            key_values.entry(key_str).or_default().push(value);
        }
    }

    // Find keys with duplicates
    let duplicate_keys: Vec<String> = key_values
        .iter()
        .filter(|(_, values)| values.len() > 1)
        .map(|(key, _)| key.clone())
        .collect();

    if duplicate_keys.is_empty() {
        return Ok(None);
    }

    // Build a new mapping with merged values
    let mut removed_fields = Vec::new();

    for key in &duplicate_keys {
        let values = &key_values[key];

        // Determine if this is a sequence field
        let is_sequence_field = SEQUENCE_FIELDS.contains(&key.as_str());

        if is_sequence_field {
            // For sequence fields, create a proper sequence using SequenceBuilder
            // Remove ALL occurrences of the duplicate key
            while mapping.remove(key.as_str()).is_some() {}

            let mut seq_builder = yaml_edit::YamlBuilder::sequence();
            for value in values {
                seq_builder = seq_builder.item(value);
            }
            let yaml_builder = seq_builder.build();
            let seq_file = yaml_builder.build();

            // Extract the sequence from the built file
            if let Some(seq_doc) = seq_file.documents().next() {
                if let Some(seq) = seq_doc.as_sequence() {
                    mapping.set(key.as_str(), seq);
                }
            }
        } else {
            // For non-sequence fields, keep the first value (already in place)
            // Just remove all other duplicate entries (skip the first)
            let entries_to_remove: Vec<_> = mapping
                .entries()
                .enumerate()
                .filter(|(i, e)| *i > 0 && e.key_matches(key.as_str()))
                .map(|(_, e)| e)
                .collect();

            for entry in entries_to_remove {
                entry.remove();
            }
        }

        // Add the field name once for each duplicate (values.len() - 1 times)
        // E.g., if there are 3 "Reference" keys, add "Reference" 2 times
        for _ in 0..(values.len() - 1) {
            removed_fields.push(key.clone());
        }
    }

    // Save the document
    doc.to_file(&metadata_path)
        .map_err(|e| FixerError::Other(format!("Failed to save YAML: {}", e)))?;

    Ok(Some(removed_fields))
}

fn check_not_mapping(base_path: &Path) -> Result<Option<usize>, FixerError> {
    let metadata_path = base_path.join("debian/upstream/metadata");

    if !metadata_path.exists() {
        return Ok(None);
    }

    let doc = yaml_edit::Document::from_file(&metadata_path)
        .map_err(|e| FixerError::Other(format!("Failed to open YAML: {}", e)))?;

    // Check if the document is a sequence instead of a mapping
    if let Some(sequence) = doc.as_sequence() {
        let items: Vec<_> = sequence.values().collect();

        // Check if it's a single-element list
        if items.len() == 1 {
            return Ok(Some(1));
        }

        // Check if all items are single-key mappings
        let all_single_key_mappings = items.iter().all(|item| {
            if let yaml_edit::YamlNode::Mapping(mapping_node) = item {
                mapping_node.entries().count() == 1
            } else {
                false
            }
        });

        if all_single_key_mappings {
            return Ok(Some(items.len()));
        }
    }

    Ok(None)
}

fn fix_not_mapping(base_path: &Path) -> Result<(), FixerError> {
    let metadata_path = base_path.join("debian/upstream/metadata");

    let doc = yaml_edit::Document::from_file(&metadata_path)
        .map_err(|e| FixerError::Other(format!("Failed to open YAML: {}", e)))?;

    let Some(sequence) = doc.as_sequence() else {
        return Ok(());
    };

    let items: Vec<_> = sequence.values().collect();

    if items.len() == 1 {
        // Single element list - unwrap it by getting its text and re-parsing
        let item_text = items[0].to_string().trim().to_string();
        std::fs::write(&metadata_path, item_text)
            .map_err(|e| FixerError::Other(format!("Failed to write file: {}", e)))?;
    } else {
        // List of single-key dicts - merge them into a single mapping
        let all_single_key_mappings = items.iter().all(|item| {
            if let yaml_edit::YamlNode::Mapping(mapping_node) = item {
                mapping_node.entries().count() == 1
            } else {
                false
            }
        });

        if all_single_key_mappings {
            // Build a new mapping and document
            let new_mapping = yaml_edit::Mapping::new();
            let new_doc = yaml_edit::Document::from_mapping(new_mapping);

            // Get the mapping from the document (they share the same underlying data)
            let doc_mapping = new_doc.as_mapping().unwrap();

            // Collect all key-value pairs from the items
            for item in items {
                if let yaml_edit::YamlNode::Mapping(mapping_node) = item {
                    for (key, value) in mapping_node.iter() {
                        if let yaml_edit::YamlNode::Scalar(key_scalar) = key {
                            let key_str = key_scalar.as_string();
                            doc_mapping.set(key_str, value);
                        }
                    }
                }
            }

            let content = doc_mapping.to_string();
            std::fs::write(&metadata_path, content)
                .map_err(|e| FixerError::Other(format!("Failed to write file: {}", e)))?;
        }
    }

    Ok(())
}

fn fix_empty_documents(base_path: &Path) -> Result<Option<String>, FixerError> {
    let metadata_path = base_path.join("debian/upstream/metadata");

    if !metadata_path.exists() {
        return Ok(None);
    }

    let yaml = yaml_edit::YamlFile::from_path(&metadata_path)
        .map_err(|e| FixerError::Other(format!("Failed to open YAML: {}", e)))?;

    let documents: Vec<yaml_edit::Document> = yaml.documents().collect();
    let mut has_empty = false;

    for doc in &documents {
        if let Some(mapping) = doc.as_mapping() {
            if mapping.entries().count() == 0 {
                has_empty = true;
                break;
            }
        } else if let Some(sequence) = doc.as_sequence() {
            if sequence.values().count() == 0 {
                has_empty = true;
                break;
            }
        } else if let Some(scalar) = doc.as_scalar() {
            let scalar_str = scalar.as_string();
            // Treat empty scalars or directives as empty
            if scalar_str.trim().is_empty() || scalar_str.trim().starts_with("%YAML") {
                has_empty = true;
                break;
            }
        } else {
            // Document with no content (just directives)
            has_empty = true;
            break;
        }
    }

    if !has_empty {
        return Ok(None);
    }

    // Filter out empty documents and keep non-empty ones
    let non_empty_docs: Vec<yaml_edit::Document> = documents
        .into_iter()
        .filter(|doc: &yaml_edit::Document| {
            if let Some(mapping) = doc.as_mapping() {
                mapping.entries().count() > 0
            } else if let Some(sequence) = doc.as_sequence() {
                sequence.values().count() > 0
            } else if let Some(scalar) = doc.as_scalar() {
                let s = scalar.as_string();
                !s.trim().is_empty() && !s.trim().starts_with("%YAML")
            } else {
                // Document with no content (just directives) - filter it out
                false
            }
        })
        .collect();

    if non_empty_docs.is_empty() {
        // All documents were empty, remove the file
        std::fs::remove_file(&metadata_path)
            .map_err(|e| FixerError::Other(format!("Failed to remove file: {}", e)))?;
        return Ok(Some(
            "Remove empty debian/upstream/metadata file.".to_string(),
        ));
    }

    // If we only have one document left, save it with any leading content preserved
    if non_empty_docs.len() == 1 {
        // Read the original file to extract leading content (comments, directives before first ---)
        let original_content = std::fs::read_to_string(&metadata_path)
            .map_err(|e| FixerError::Other(format!("Failed to read file: {}", e)))?;

        let leading_content = if let Some(pos) = original_content.find("---") {
            &original_content[..pos]
        } else {
            ""
        };

        // Save the document and prepend leading content if it exists
        let doc_content = non_empty_docs[0].to_string();
        let final_content = if !leading_content.trim().is_empty() {
            format!("{}{}", leading_content, doc_content)
        } else {
            doc_content
        };

        std::fs::write(&metadata_path, final_content)
            .map_err(|e| FixerError::Other(format!("Failed to write file: {}", e)))?;
        return Ok(Some(
            "Discard extra empty YAML documents in debian/upstream/metadata.".to_string(),
        ));
    }

    // Multiple non-empty documents - just save the first one with leading content
    if let Some(doc) = non_empty_docs.first() {
        // Read the original file to extract leading content
        let original_content = std::fs::read_to_string(&metadata_path)
            .map_err(|e| FixerError::Other(format!("Failed to read file: {}", e)))?;

        let leading_content = if let Some(pos) = original_content.find("---") {
            &original_content[..pos]
        } else {
            ""
        };

        let doc_content = doc.to_string();
        let final_content = if !leading_content.trim().is_empty() {
            format!("{}{}", leading_content, doc_content)
        } else {
            doc_content
        };

        std::fs::write(&metadata_path, final_content)
            .map_err(|e| FixerError::Other(format!("Failed to write file: {}", e)))?;
    }

    Ok(Some(
        "Discard extra empty YAML documents in debian/upstream/metadata.".to_string(),
    ))
}

pub fn run(base_path: &Path, _preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let metadata_path = base_path.join("debian/upstream/metadata");

    if !metadata_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let mut descriptions = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Check if we should fix upstream-metadata-yaml-invalid
    let yaml_invalid_issue = crate::LintianIssue::source("upstream-metadata-yaml-invalid");
    let yaml_not_mapping_issue = crate::LintianIssue::source("upstream-metadata-not-yaml-mapping");

    // Try to fix duplicate keys first (fixes upstream-metadata-yaml-invalid)
    if yaml_invalid_issue.should_fix(base_path) {
        match fix_duplicate_keys(base_path) {
            Ok(Some(fields)) => {
                let mut sorted_fields = fields;
                sorted_fields.sort();
                let desc = format!(
                    "Remove duplicate values for fields {} in debian/upstream/metadata.",
                    sorted_fields.join(", ")
                );
                descriptions.push(desc);
                fixed_issues.push(yaml_invalid_issue.clone());
            }
            Ok(None) => {}
            Err(e) => {
                debug!("Failed to fix duplicate keys: {}", e);
            }
        }
    } else {
        overridden_issues.push(yaml_invalid_issue);
    }

    // Check for structure issues (not a mapping)
    match check_not_mapping(base_path) {
        Ok(Some(count)) => {
            // Check if each issue should be fixed
            let mut issues_to_fix = 0;
            let mut issues_overridden = 0;

            for _ in 0..count {
                if yaml_not_mapping_issue.should_fix(base_path) {
                    issues_to_fix += 1;
                } else {
                    issues_overridden += 1;
                }
            }

            if issues_to_fix > 0 {
                // Apply the fix
                match fix_not_mapping(base_path) {
                    Ok(()) => {
                        descriptions
                            .push("Use YAML mapping in debian/upstream/metadata.".to_string());
                        // Report one issue for each item that was fixed
                        for _ in 0..issues_to_fix {
                            fixed_issues.push(yaml_not_mapping_issue.clone());
                        }
                    }
                    Err(e) => {
                        debug!("Failed to fix mapping structure: {}", e);
                    }
                }
            }

            // Report overridden issues
            for _ in 0..issues_overridden {
                overridden_issues.push(yaml_not_mapping_issue.clone());
            }
        }
        Ok(None) => {}
        Err(e) => {
            debug!("Failed to check mapping structure: {}", e);
        }
    }

    // Try to fix empty documents (also related to yaml-invalid)
    match fix_empty_documents(base_path) {
        Ok(Some(desc)) => descriptions.push(desc),
        Ok(None) => {}
        Err(e) => {
            debug!("Failed to fix empty documents: {}", e);
        }
    }

    if descriptions.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    let description = descriptions.join(" ");

    Ok(crate::FixerResult::builder(description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "upstream-metadata-invalid",
    tags: [],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}
