use crate::upstream_metadata::VALID_FIELD_NAMES;
use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use strsim::levenshtein;

pub fn run(base_path: &Path, _preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let metadata_path = base_path.join("debian/upstream/metadata");

    if !metadata_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let contents = fs::read_to_string(&metadata_path)?;
    let mut yaml: serde_yaml::Value = serde_yaml::from_str(&contents)
        .map_err(|e| FixerError::Other(format!("Failed to parse YAML: {}", e)))?;

    let valid_fields: HashSet<&str> = VALID_FIELD_NAMES.iter().copied().collect();
    let mut typo_fixed = Vec::new();
    let mut case_fixed = Vec::new();

    if let serde_yaml::Value::Mapping(ref mut map) = yaml {
        let keys: Vec<String> = map
            .keys()
            .filter_map(|k| k.as_str().map(|s| s.to_string()))
            .collect();

        for field in keys {
            if valid_fields.contains(field.as_str()) {
                continue;
            }

            // Handle X- prefix
            if let Some(without_prefix) = field.strip_prefix("X-") {
                if valid_fields.contains(without_prefix) {
                    if map.contains_key(serde_yaml::Value::String(without_prefix.to_string())) {
                        // Both exist, warn and skip
                        eprintln!("Warning: Both {} and {} exist.", field, without_prefix);
                        continue;
                    }

                    if let Some(value) = map.remove(serde_yaml::Value::String(field.clone())) {
                        map.insert(serde_yaml::Value::String(without_prefix.to_string()), value);
                        typo_fixed.push((field.clone(), without_prefix.to_string()));
                    }
                    continue;
                }
            }

            // Check for typos using Levenshtein distance
            for &option in VALID_FIELD_NAMES {
                if levenshtein(&field, option) == 1 {
                    if let Some(value) = map.remove(serde_yaml::Value::String(field.clone())) {
                        map.insert(serde_yaml::Value::String(option.to_string()), value);

                        if option.to_lowercase() == field.to_lowercase() {
                            case_fixed.push((field.clone(), option.to_string()));
                        } else {
                            typo_fixed.push((field.clone(), option.to_string()));
                        }
                    }
                    break;
                }
            }
        }
    }

    if typo_fixed.is_empty() && case_fixed.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Write back the YAML
    let output = serde_yaml::to_string(&yaml)
        .map_err(|e| FixerError::Other(format!("Failed to serialize YAML: {}", e)))?;
    fs::write(&metadata_path, output)?;

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
