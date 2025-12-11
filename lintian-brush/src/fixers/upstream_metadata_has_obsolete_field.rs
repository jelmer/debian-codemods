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

    let contents = fs::read_to_string(&metadata_path)?;
    let mut yaml: serde_yaml::Value = serde_yaml::from_str(&contents)
        .map_err(|e| FixerError::Other(format!("Failed to parse YAML: {}", e)))?;

    let Some(map) = yaml.as_mapping_mut() else {
        return Err(FixerError::Other(
            "YAML document is not a mapping".to_string(),
        ));
    };

    let mut obsolete_fields: HashMap<String, String> = HashMap::new();
    let mut removed_fields: Vec<String> = Vec::new();

    // Check if Name or Contact fields exist in the metadata
    let has_name_or_contact = map.contains_key(serde_yaml::Value::String("Name".to_string()))
        || map.contains_key(serde_yaml::Value::String("Contact".to_string()));

    // If the debian/copyright file is machine-readable, then we can drop the
    // Name/Contact information from the debian/upstream/metadata file.
    if has_name_or_contact {
        let copyright_path = base_path.join("debian/copyright");
        obsolete_fields = upstream_fields_in_copyright(&copyright_path);
    }

    // First pass: remove null and empty fields
    for field in ["Name", "Contact"] {
        let Some(value) = map.get(serde_yaml::Value::String(field.to_string())) else {
            continue;
        };

        let is_empty_or_null =
            value.is_null() || value.as_str().map(|s| s.trim().is_empty()).unwrap_or(false);

        if is_empty_or_null {
            map.remove(serde_yaml::Value::String(field.to_string()));
            removed_fields.push(field.to_string());
        }
    }

    // Second pass: check for obsolete fields
    for (field, copyright_value) in &obsolete_fields {
        let Some(um_value) = map.get(serde_yaml::Value::String(field.clone())) else {
            continue;
        };

        let Some(um_str) = um_value.as_str() else {
            continue;
        };

        // Split both values by separator characters and compare as sets
        let copyright_entries: HashSet<String> = split_sep_chars(copyright_value)
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        let um_entries: HashSet<String> = split_sep_chars(um_str)
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        if copyright_entries != um_entries {
            continue;
        }

        map.remove(serde_yaml::Value::String(field.clone()));
        if !removed_fields.contains(field) {
            removed_fields.push(field.clone());
        }
    }

    if removed_fields.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // If only addon-only fields remain, clear the file
    let remaining_keys: HashSet<String> = map
        .keys()
        .filter_map(|k| k.as_str().map(|s| s.to_string()))
        .collect();

    let addon_only_set: HashSet<String> =
        ADDON_ONLY_FIELDS.iter().map(|&s| s.to_string()).collect();

    if remaining_keys.difference(&addon_only_set).count() == 0 {
        map.clear();
        // If the file is now empty, remove it and the upstream directory if empty
        std::fs::remove_file(&metadata_path)?;
        if let Some(parent) = metadata_path.parent() {
            let _ = std::fs::remove_dir(parent); // Ignore error if dir not empty
        }
    } else {
        // Write back the YAML
        let output = serde_yaml::to_string(&yaml)
            .map_err(|e| FixerError::Other(format!("Failed to serialize YAML: {}", e)))?;
        fs::write(&metadata_path, output)?;
    }

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
        let yaml: serde_yaml::Value = serde_yaml::from_str(&updated).unwrap();
        assert!(yaml.get("Name").is_some());
        assert!(yaml.get("Contact").is_none());
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
        let yaml: serde_yaml::Value = serde_yaml::from_str(&updated).unwrap();
        assert!(yaml.get("Name").is_none());
        assert!(yaml.get("Contact").is_none());
        assert!(yaml.get("Repository").is_some());
    }
}
