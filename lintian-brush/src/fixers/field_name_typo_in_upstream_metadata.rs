use crate::diagnostic::{Action, Diagnostic, YamlAction};
use crate::upstream_metadata::DEP12_FIELD_ORDER;
use crate::FixerError;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use strsim::levenshtein;

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let rel = PathBuf::from("debian/upstream/metadata");
    let abs = base_path.join(&rel);
    if !abs.exists() {
        return Ok(Vec::new());
    }

    let valid_fields: HashSet<&str> = DEP12_FIELD_ORDER.iter().copied().collect();
    let mut diagnostics = Vec::new();

    let doc = yaml_edit::Document::from_file(&abs)
        .map_err(|e| FixerError::Other(format!("Failed to open YAML: {}", e)))?;
    let Some(mapping) = doc.as_mapping() else {
        return Ok(Vec::new());
    };

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

        // X- prefix: drop it if the unprefixed name is valid.
        if let Some(without_prefix) = field.strip_prefix("X-") {
            if valid_fields.contains(without_prefix) {
                if mapping.contains_key(without_prefix) {
                    eprintln!("Warning: Both {} and {} exist.", field, without_prefix);
                    continue;
                }
                diagnostics.push(crate::diagnostic::Diagnostic::untagged(
                    format!("typo\t{} ⇒ {}", field, without_prefix),
                    vec![Action::Yaml(YamlAction::RenameField {
                        file: rel.clone(),
                        parent_path: Vec::new(),
                        from: field.clone(),
                        to: without_prefix.to_string(),
                    })],
                ));
                continue;
            }
        }

        // Levenshtein distance == 1 from a valid field name.
        let Some(target) = DEP12_FIELD_ORDER
            .iter()
            .copied()
            .find(|v| levenshtein(&field, v) == 1)
        else {
            continue;
        };
        let is_case = target.eq_ignore_ascii_case(&field);
        let message = format!(
            "{}\t{} ⇒ {}",
            if is_case { "case" } else { "typo" },
            field,
            target,
        );
        diagnostics.push(crate::diagnostic::Diagnostic::untagged(
            message,
            vec![Action::Yaml(YamlAction::RenameField {
                file: rel.clone(),
                parent_path: Vec::new(),
                from: field,
                to: target.to_string(),
            })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let mut case_pairs: Vec<(String, String)> = Vec::new();
    let mut typo_pairs: Vec<(String, String)> = Vec::new();
    for diag in fixed {
        let Some((kind, rest)) = diag.message.split_once('\t') else {
            continue;
        };
        let Some((old, new)) = rest.split_once(" ⇒ ") else {
            continue;
        };
        let pair = (old.to_string(), new.to_string());
        if kind == "case" {
            case_pairs.push(pair);
        } else {
            typo_pairs.push(pair);
        }
    }

    let mut kind = String::new();
    if !case_pairs.is_empty() {
        kind.push_str("case");
        if case_pairs.len() > 1 {
            kind.push('s');
        }
    }
    if !typo_pairs.is_empty() {
        if !case_pairs.is_empty() {
            kind.push_str(" and ");
        }
        kind.push_str("typo");
        if typo_pairs.len() > 1 {
            kind.push('s');
        }
    }

    let mut all = case_pairs;
    all.extend(typo_pairs);
    all.sort();
    let fixed_str = all
        .iter()
        .map(|(old, new)| format!("{} ⇒ {}", old, new))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "Fix field name {} in debian/upstream/metadata ({}).",
        kind, fixed_str
    )
}

declare_fixer! {
    name: "field-name-typo-in-upstream-metadata",
    tags: [],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    },
    describe: |fixed, actions| {
        describe_aggregate(fixed, actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("a", "a"), 0);
        assert_eq!(levenshtein("a", "b"), 1);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "abd"), 1);
        assert_eq!(levenshtein("Repository", "Repositoryz"), 1);
    }

    #[test]
    fn test_typo_fix() {
        let tmp = TempDir::new().unwrap();
        let metadata_dir = tmp.path().join("debian/upstream");
        fs::create_dir_all(&metadata_dir).unwrap();
        let path = metadata_dir.join("metadata");
        fs::write(&path, "Name: foo\nRepositry: https://example.org/foo\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Fix field name typo in debian/upstream/metadata (Repositry ⇒ Repository)."
        );

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Name: foo\nRepository: https://example.org/foo\n",
        );
    }

    #[test]
    fn test_x_prefix() {
        let tmp = TempDir::new().unwrap();
        let metadata_dir = tmp.path().join("debian/upstream");
        fs::create_dir_all(&metadata_dir).unwrap();
        let path = metadata_dir.join("metadata");
        fs::write(&path, "Name: foo\nX-Repository: https://example.org/foo\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Fix field name typo in debian/upstream/metadata (X-Repository ⇒ Repository)."
        );

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Name: foo\nRepository: https://example.org/foo\n",
        );
    }

    #[test]
    fn test_no_changes_needed() {
        let tmp = TempDir::new().unwrap();
        let metadata_dir = tmp.path().join("debian/upstream");
        fs::create_dir_all(&metadata_dir).unwrap();
        let path = metadata_dir.join("metadata");
        let original = "Name: foo\nRepository: https://example.org/foo\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
