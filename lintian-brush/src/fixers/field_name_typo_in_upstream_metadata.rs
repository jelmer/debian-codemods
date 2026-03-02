use crate::upstream_metadata::DEP12_FIELD_ORDER;
use crate::{FixerError, FixerPreferences, FixerResult};
use std::collections::HashSet;
use std::path::Path;
use strsim::levenshtein;

pub fn run(base_path: &Path, _preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let metadata_path = base_path.join("debian/upstream/metadata");

    if !metadata_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let valid_fields: HashSet<&str> = DEP12_FIELD_ORDER.iter().copied().collect();
    let mut typo_fixed = Vec::new();
    let mut case_fixed = Vec::new();

    let mut updater = yaml_edit::YamlUpdater::new(&metadata_path)
        .map_err(|e| FixerError::Other(format!("Failed to create YAML updater: {}", e)))?;

    let doc = updater
        .open()
        .map_err(|e| FixerError::Other(format!("Failed to open YAML: {}", e)))?;

    // Get all keys from the YAML
    let keys: Vec<String> = doc
        .keys()
        .map_err(|e| FixerError::Other(format!("Failed to get keys: {}", e)))?;

    for field in keys {
        if valid_fields.contains(field.as_str()) {
            continue;
        }

        // Handle X- prefix
        if let Some(without_prefix) = field.strip_prefix("X-") {
            if valid_fields.contains(without_prefix) {
                let target_exists = doc
                    .contains_key(without_prefix)
                    .map_err(|e| FixerError::Other(format!("Failed to check key: {}", e)))?;

                if target_exists {
                    // Both exist, warn and skip
                    eprintln!("Warning: Both {} and {} exist.", field, without_prefix);
                    continue;
                }

                let value = doc
                    .remove(&field)
                    .map_err(|e| FixerError::Other(format!("Failed to remove key: {}", e)))?
                    .expect("Key should exist");

                doc.set(without_prefix, value)
                    .map_err(|e| FixerError::Other(format!("Failed to set key: {}", e)))?;

                typo_fixed.push((field.clone(), without_prefix.to_string()));
                continue;
            }
        }

        // Check for typos using Levenshtein distance
        for &option in DEP12_FIELD_ORDER {
            if levenshtein(&field, option) == 1 {
                let value = doc
                    .remove(&field)
                    .map_err(|e| FixerError::Other(format!("Failed to remove key: {}", e)))?
                    .expect("Key should exist");

                doc.set(option, value)
                    .map_err(|e| FixerError::Other(format!("Failed to set key: {}", e)))?;

                if option.to_lowercase() == field.to_lowercase() {
                    case_fixed.push((field.clone(), option.to_string()));
                } else {
                    typo_fixed.push((field.clone(), option.to_string()));
                }
                break;
            }
        }
    }

    if typo_fixed.is_empty() && case_fixed.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Close updater to save changes
    updater
        .close()
        .map_err(|e| FixerError::Other(format!("Failed to close YAML: {}", e)))?;

    // Build description message
    let mut kind = String::new();
    if !case_fixed.is_empty() {
        kind.push_str("case");
        if case_fixed.len() > 1 {
            kind.push('s');
        }
    }
    if !typo_fixed.is_empty() {
        if !case_fixed.is_empty() {
            kind.push_str(" and ");
        }
        kind.push_str("typo");
        if typo_fixed.len() > 1 {
            kind.push('s');
        }
    }

    let mut all_fixed = case_fixed.clone();
    all_fixed.extend(typo_fixed.clone());
    all_fixed.sort();

    let fixed_str = all_fixed
        .iter()
        .map(|(old, new)| format!("{} ⇒ {}", old, new))
        .collect::<Vec<_>>()
        .join(", ");

    let description = format!(
        "Fix field name {} in debian/upstream/metadata ({}).",
        kind, fixed_str
    );

    Ok(FixerResult::builder(description).build())
}

declare_fixer! {
    name: "field-name-typo-in-upstream-metadata",
    tags: [],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("a", "a"), 0);
        assert_eq!(levenshtein("a", "b"), 1);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "abd"), 1);
        assert_eq!(levenshtein("Repository", "Repositoryz"), 1);
        assert_eq!(levenshtein("Repository:", "Repository"), 1);
        assert_eq!(levenshtein("abc", "abcd"), 1);
        assert_eq!(levenshtein("abc", "ab"), 1);
        assert_eq!(levenshtein("abc", "xyz"), 3);
    }
}
