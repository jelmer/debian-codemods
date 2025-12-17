use crate::upstream_metadata::ADDON_ONLY_FIELDS;
use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult};
use debian_copyright::lossless::Copyright;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Extract upstream fields from a copyright file
///
/// Returns a HashMap mapping field names ("Name", "Contact") to their values
fn upstream_fields_in_copyright(copyright_path: &Path) -> HashMap<String, String> {
    let mut result = HashMap::new();

    let Ok(content) = fs::read_to_string(copyright_path) else {
        return result;
    };

    let Ok(copyright) = content.parse::<Copyright>() else {
        return result;
    };

    let Some(header) = copyright.header() else {
        return result;
    };

    if let Some(name) = header.upstream_name() {
        result.insert("Name".to_string(), name.to_string());
    }
    if let Some(contact) = header.upstream_contact() {
        result.insert("Contact".to_string(), contact.to_string());
    }

    result
}

/// Split a value by separator characters (newlines, multiple spaces, tabs)
fn split_sep_chars(value: &str) -> Vec<String> {
    let sep_regex = Regex::new(r"\n+|\s\s+|\t+").unwrap();
    sep_regex
        .split(value)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn run(base_path: &Path, _preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let metadata_path = base_path.join("debian/upstream/metadata");

    if !metadata_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let mut obsolete_fields: HashMap<String, String> = HashMap::new();
    let mut removed_fields: Vec<String> = Vec::new();

    let mut updater = yaml_edit::YamlUpdater::new(&metadata_path)
        .map_err(|e| FixerError::Other(format!("Failed to create YAML updater: {}", e)))?;

    let doc = updater
        .open()
        .map_err(|e| FixerError::Other(format!("Failed to open YAML: {}", e)))?;

    // Check if Name or Contact fields exist in the metadata
    let has_name_or_contact =
        doc.contains_key("Name").unwrap_or(false) || doc.contains_key("Contact").unwrap_or(false);

    // If the debian/copyright file is machine-readable, then we can drop the
    // Name/Contact information from the debian/upstream/metadata file.
    if has_name_or_contact {
        let copyright_path = base_path.join("debian/copyright");
        obsolete_fields = upstream_fields_in_copyright(&copyright_path);
    }

    // First pass: remove null and empty fields
    for field in ["Name", "Contact"] {
        let value_opt = doc
            .get(field)
            .map_err(|e| FixerError::Other(format!("Failed to get field {}: {}", field, e)))?;

        if let Some(value) = value_opt {
            let is_empty_or_null = match value {
                yaml_edit::Value::Null => true,
                yaml_edit::Value::String(s) => s.trim().is_empty(),
                _ => false,
            };

            if is_empty_or_null {
                doc.remove(field).map_err(|e| {
                    FixerError::Other(format!("Failed to remove field {}: {}", field, e))
                })?;
                removed_fields.push(field.to_string());
            }
        }
    }

    // Second pass: check for obsolete fields
    for (field, copyright_value) in &obsolete_fields {
        let um_value_opt = doc
            .get(field)
            .map_err(|e| FixerError::Other(format!("Failed to get field {}: {}", field, e)))?;

        let Some(um_value) = um_value_opt else {
            continue;
        };

        let um_str = match um_value {
            yaml_edit::Value::String(s) => s,
            _ => continue,
        };

        // Split both values by separator characters and compare as sets
        let copyright_entries: HashSet<String> = split_sep_chars(copyright_value)
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        let um_entries: HashSet<String> = split_sep_chars(&um_str)
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        if copyright_entries != um_entries {
            continue;
        }

        doc.remove(field)
            .map_err(|e| FixerError::Other(format!("Failed to remove field {}: {}", field, e)))?;
        if !removed_fields.contains(field) {
            removed_fields.push(field.clone());
        }
    }

    if removed_fields.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // If only addon-only fields remain, clear the file
    let remaining_keys: HashSet<String> = doc
        .keys()
        .map_err(|e| FixerError::Other(format!("Failed to get keys: {}", e)))?
        .into_iter()
        .collect();

    let addon_only_set: HashSet<String> =
        ADDON_ONLY_FIELDS.iter().map(|&s| s.to_string()).collect();

    if remaining_keys.difference(&addon_only_set).count() == 0 {
        // Clear the file - the YamlUpdater will remove the file and directory
        // automatically when closed since it has remove_empty=True by default
        doc.clear()
            .map_err(|e| FixerError::Other(format!("Failed to clear YAML: {}", e)))?;
    }

    // Close updater to save changes (or remove file if empty)
    updater
        .close()
        .map_err(|e| FixerError::Other(format!("Failed to close YAML: {}", e)))?;

    removed_fields.sort();

    let description = format!(
        "Remove obsolete field{} {} from debian/upstream/metadata (already present in machine-readable debian/copyright).",
        if removed_fields.len() > 1 { "s" } else { "" },
        removed_fields.join(", ")
    );

    Ok(FixerResult::builder(description).build())
}

declare_fixer! {
    name: "upstream-metadata-has-obsolete-field",
    tags: [],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_no_metadata_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_remove_null_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir_all(&upstream_dir).unwrap();

        let metadata_content = r#"Name: test-package
Contact: null
"#;
        fs::write(upstream_dir.join("metadata"), metadata_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        let updated = fs::read_to_string(upstream_dir.join("metadata")).unwrap();
        assert_eq!(updated, "Name: test-package\n");
    }

    #[test]
    fn test_remove_obsolete_field_from_copyright() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir_all(&upstream_dir).unwrap();

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

        let metadata_content = r#"Name: test-package
Contact: Test User <test@example.com>
Repository: https://github.com/example/test
"#;
        fs::write(upstream_dir.join("metadata"), metadata_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        let updated = fs::read_to_string(upstream_dir.join("metadata")).unwrap();
        assert_eq!(updated, "Repository: https://github.com/example/test\n");
    }
}
