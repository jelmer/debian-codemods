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

    let doc = yaml_edit::Document::from_file(&metadata_path)
        .map_err(|e| FixerError::Other(format!("Failed to open YAML: {}", e)))?;

    let Some(mapping) = doc.as_mapping() else {
        return Err(FixerError::NoChanges);
    };

    // Get all keys from the YAML
    let keys: Vec<String> = mapping
        .keys()
        .filter_map(|node| match node {
            yaml_edit::YamlNode::Scalar(scalar) => Some(scalar.as_string()),
            _ => None,
        })
        .collect();

    for field in keys {
        if valid_fields.contains(field.as_str()) {
            continue;
        }

        // Handle X- prefix
        if let Some(without_prefix) = field.strip_prefix("X-") {
            if valid_fields.contains(without_prefix) {
                let target_exists = mapping.keys().any(|k| k == without_prefix);

                if target_exists {
                    // Both exist, warn and skip
                    eprintln!("Warning: Both {} and {} exist.", field, without_prefix);
                    continue;
                }

                let value = mapping.get(field.as_str()).ok_or_else(|| {
                    FixerError::Other(format!("Failed to get value for key: {}", field))
                })?;

                mapping.remove(field.as_str());
                mapping.set(without_prefix, value);

                typo_fixed.push((field.clone(), without_prefix.to_string()));
                continue;
            }
        }

        // Check for typos using Levenshtein distance
        for &option in DEP12_FIELD_ORDER {
            if levenshtein(&field, option) == 1 {
                let value = mapping.get(field.as_str()).ok_or_else(|| {
                    FixerError::Other(format!("Failed to get value for key: {}", field))
                })?;

                mapping.remove(field.as_str());
                mapping.set(option, value);

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

    // Save changes
    doc.to_file(&metadata_path)
        .map_err(|e| FixerError::Other(format!("Failed to save YAML: {}", e)))?;

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
