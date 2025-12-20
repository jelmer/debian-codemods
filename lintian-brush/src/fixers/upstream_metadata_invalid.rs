use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult};
use indexmap::IndexMap;
use log::warn;
use std::path::Path;
use yaml_edit::Value;

const SEQUENCE_FIELDS: &[&str] = &["Reference", "Screenshots"];

fn fix_duplicate_keys(base_path: &Path) -> Result<Option<Vec<String>>, FixerError> {
    let metadata_path = base_path.join("debian/upstream/metadata");

    if !metadata_path.exists() {
        return Ok(None);
    }

    // Merge duplicates directly at the node level (preserves formatting like "01")
    let removed_fields = yaml_edit::merge_duplicate_keys_in_place(&metadata_path, SEQUENCE_FIELDS)
        .map_err(|e| FixerError::Other(format!("Failed to merge duplicate keys: {}", e)))?;

    Ok(removed_fields)
}

fn check_not_mapping(base_path: &Path) -> Result<Option<usize>, FixerError> {
    let metadata_path = base_path.join("debian/upstream/metadata");

    if !metadata_path.exists() {
        return Ok(None);
    }

    let mut updater = yaml_edit::YamlUpdater::new(&metadata_path)
        .map_err(|e| FixerError::Other(format!("Failed to create updater: {}", e)))?;

    let doc = updater
        .open()
        .map_err(|e| FixerError::Other(format!("Failed to open: {}", e)))?;

    let is_list = doc
        .is_list()
        .map_err(|e| FixerError::Other(format!("Failed to check if list: {}", e)))?;

    if !is_list {
        return Ok(None);
    }

    let all_value = doc
        .get_all()
        .map_err(|e| FixerError::Other(format!("Failed to get all: {}", e)))?;

    let count = match all_value {
        Value::List(ref items) if items.len() == 1 => 1,
        Value::List(ref items)
            if items
                .iter()
                .all(|item| matches!(item, Value::Map(m) if m.len() == 1)) =>
        {
            items.len()
        }
        _ => 0,
    };

    Ok(if count > 0 { Some(count) } else { None })
}

fn fix_not_mapping(base_path: &Path) -> Result<(), FixerError> {
    let metadata_path = base_path.join("debian/upstream/metadata");

    let mut updater = yaml_edit::YamlUpdater::new(&metadata_path)
        .map_err(|e| FixerError::Other(format!("Failed to create updater: {}", e)))?;

    let doc = updater
        .open()
        .map_err(|e| FixerError::Other(format!("Failed to open: {}", e)))?;

    let all_value = doc
        .get_all()
        .map_err(|e| FixerError::Other(format!("Failed to get all: {}", e)))?;

    match all_value {
        Value::List(ref items) if items.len() == 1 => {
            // Single element list - unwrap it
            doc.set_all(items[0].clone())
                .map_err(|e| FixerError::Other(format!("Failed to set all: {}", e)))?;
        }
        Value::List(ref items)
            if items
                .iter()
                .all(|item| matches!(item, Value::Map(m) if m.len() == 1)) =>
        {
            // List of single-key dicts - merge them
            let mut merged = IndexMap::new();
            for item in items {
                if let Value::Map(m) = item {
                    merged.extend(m.clone());
                }
            }
            doc.set_all(Value::Map(merged))
                .map_err(|e| FixerError::Other(format!("Failed to set all: {}", e)))?;
        }
        _ => {}
    };

    updater
        .close()
        .map_err(|e| FixerError::Other(format!("Failed to close: {}", e)))?;

    Ok(())
}

fn fix_empty_documents(base_path: &Path) -> Result<Option<String>, FixerError> {
    let metadata_path = base_path.join("debian/upstream/metadata");

    if !metadata_path.exists() {
        return Ok(None);
    }

    let mut updater = yaml_edit::MultiYamlUpdater::new(&metadata_path)
        .map_err(|e| FixerError::Other(format!("Failed to create multi updater: {}", e)))?;

    let doc = updater
        .open()
        .map_err(|e| FixerError::Other(format!("Failed to open: {}", e)))?;

    let len = doc
        .len()
        .map_err(|e| FixerError::Other(format!("Failed to get length: {}", e)))?;

    let mut to_remove = Vec::new();
    for i in 0..len {
        let item = doc
            .get(i)
            .map_err(|e| FixerError::Other(format!("Failed to get item: {}", e)))?;

        if let Some(value) = item {
            let is_empty = match value {
                Value::Null => true,
                Value::Map(ref m) => m.is_empty(),
                Value::List(ref l) => l.is_empty(),
                Value::String(ref s) => s.is_empty(),
                _ => false,
            };
            if is_empty {
                to_remove.push(i);
            }
        }
    }

    if to_remove.is_empty() {
        return Ok(None);
    }

    // Remove in reverse order
    for i in to_remove.iter().rev() {
        doc.remove(*i)
            .map_err(|e| FixerError::Other(format!("Failed to remove: {}", e)))?;
    }

    updater
        .close()
        .map_err(|e| FixerError::Other(format!("Failed to close: {}", e)))?;

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
                warn!("Failed to fix duplicate keys: {}", e);
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
                        warn!("Failed to fix mapping structure: {}", e);
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
            warn!("Failed to check mapping structure: {}", e);
        }
    }

    // Try to fix empty documents (also related to yaml-invalid)
    match fix_empty_documents(base_path) {
        Ok(Some(desc)) => descriptions.push(desc),
        Ok(None) => {}
        Err(e) => {
            warn!("Failed to fix empty documents: {}", e);
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
