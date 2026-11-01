use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, YamlAction};
use crate::upstream_metadata::DEP12_FIELD_ORDER;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::collections::HashSet;
use std::path::PathBuf;
use strsim::levenshtein;

/// Marker in the label used by `describe_aggregate` to tell the three
/// diagnostic kinds apart. Not shown to users.
const KIND_CASE: &str = "case";
const KIND_TYPO: &str = "typo";
const KIND_UNKNOWN_PREFIX: &str = "unknown-prefix";
const KIND_UNKNOWN_REMOVE: &str = "unknown-remove";

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rel = PathBuf::from("debian/upstream/metadata");
    let yaml = match ws.parsed_upstream_metadata() {
        Ok(y) => y,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };
    let Some(doc) = yaml.documents().next() else {
        return Ok(Vec::new());
    };
    let Some(mapping) = doc.as_mapping() else {
        return Ok(Vec::new());
    };

    let valid: HashSet<&str> = DEP12_FIELD_ORDER.iter().copied().collect();
    let mut diagnostics = Vec::new();

    let keys: Vec<String> = mapping
        .keys()
        .filter_map(|node| match node {
            yaml_edit::YamlNode::Scalar(scalar) => Some(scalar.as_string()),
            _ => None,
        })
        .collect();

    for field in keys {
        if valid.contains(field.as_str()) {
            continue;
        }

        // DEP-12 reserves `X-*` for user extensions. If the unprefixed
        // name is valid, strip the prefix (the user accidentally
        // extension-marked a real field). Otherwise it's an intentional
        // extension and we leave it alone, even though lintian still
        // flags it.
        if let Some(without_prefix) = field.strip_prefix("X-") {
            if valid.contains(without_prefix) {
                if mapping.contains_key(without_prefix) {
                    eprintln!("Warning: Both {} and {} exist.", field, without_prefix);
                    continue;
                }
                diagnostics.push(Diagnostic::untagged(
                    format!(
                        "Field name {} appears to be a typo for {}.",
                        field, without_prefix
                    ),
                    format!("{}\t{} ⇒ {}", KIND_TYPO, field, without_prefix),
                    vec![Action::Yaml(YamlAction::RenameField {
                        file: rel.clone(),
                        parent_path: Vec::new(),
                        from: field.clone(),
                        to: without_prefix.to_string(),
                    })],
                ));
            }
            continue;
        }

        // Levenshtein distance == 1 from a valid field name.
        if let Some(target) = DEP12_FIELD_ORDER
            .iter()
            .copied()
            .find(|v| levenshtein(&field, v) == 1)
        {
            let is_case = target.eq_ignore_ascii_case(&field);
            let kind = if is_case { KIND_CASE } else { KIND_TYPO };
            let description = if is_case {
                format!(
                    "Field name {} has wrong case (should be {}).",
                    field, target
                )
            } else {
                format!("Field name {} appears to be a typo for {}.", field, target)
            };
            diagnostics.push(Diagnostic::untagged(
                description,
                format!("{}\t{} ⇒ {}", kind, field, target),
                vec![Action::Yaml(YamlAction::RenameField {
                    file: rel.clone(),
                    parent_path: Vec::new(),
                    from: field,
                    to: target.to_string(),
                })],
            ));
            continue;
        }

        // Not a typo of anything we know, and not X-prefixed (those
        // were handled above). Offer two opinionated plans: the safe
        // one renames to `X-<field>` (moving it into the DEP-12
        // extension namespace), the aggressive one removes the field.
        // Both are opinionated: by default we don't rewrite or drop
        // user metadata we don't understand. The rename plan is
        // suppressed if `X-<field>` would collide with an existing key.
        let issue = LintianIssue::source_with_info(
            "upstream-metadata-field-unknown",
            Visibility::Warning,
            vec![field.clone(), "[debian/upstream/metadata]".to_string()],
        );
        let description = format!("debian/upstream/metadata has unknown field {}.", field);
        let prefixed = format!("X-{}", field);
        let mut plans: Vec<ActionPlan> = Vec::new();
        if !mapping.contains_key(prefixed.as_str()) {
            plans.push(ActionPlan {
                label: format!("{}\t{} ⇒ {}", KIND_UNKNOWN_PREFIX, field, prefixed),
                opinionated: true,
                certainty: Some(Certainty::Possible),
                actions: vec![Action::Yaml(YamlAction::RenameField {
                    file: rel.clone(),
                    parent_path: Vec::new(),
                    from: field.clone(),
                    to: prefixed,
                })],
            });
        }
        plans.push(ActionPlan {
            label: format!("{}\t{}", KIND_UNKNOWN_REMOVE, field),
            opinionated: true,
            certainty: Some(Certainty::Possible),
            actions: vec![Action::Yaml(YamlAction::RemoveField {
                file: rel.clone(),
                parent_path: Vec::new(),
                key: field,
            })],
        });
        diagnostics.push(Diagnostic::with_plans(issue, description, plans));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let mut case_pairs: Vec<(String, String)> = Vec::new();
    let mut typo_pairs: Vec<(String, String)> = Vec::new();
    let mut prefix_pairs: Vec<(String, String)> = Vec::new();
    let mut removed_fields: Vec<String> = Vec::new();
    for (_, plan) in fixed {
        let Some((kind, rest)) = plan.label.split_once('\t') else {
            continue;
        };
        match kind {
            KIND_CASE | KIND_TYPO | KIND_UNKNOWN_PREFIX => {
                let Some((old, new)) = rest.split_once(" ⇒ ") else {
                    continue;
                };
                let pair = (old.to_string(), new.to_string());
                match kind {
                    KIND_CASE => case_pairs.push(pair),
                    KIND_TYPO => typo_pairs.push(pair),
                    _ => prefix_pairs.push(pair),
                }
            }
            KIND_UNKNOWN_REMOVE => removed_fields.push(rest.to_string()),
            _ => {}
        }
    }

    // The rename-plus-removal combination is awkward to phrase in one
    // sentence; emit each class on its own line. The X-prefix rename
    // only kicks in under `opinionated`, so it rarely coincides with
    // the always-safe case/typo renames.
    let mut lines: Vec<String> = Vec::new();

    if !case_pairs.is_empty() || !typo_pairs.is_empty() {
        let mut kind = String::new();
        if !case_pairs.is_empty() {
            kind.push_str(KIND_CASE);
            if case_pairs.len() > 1 {
                kind.push('s');
            }
        }
        if !typo_pairs.is_empty() {
            if !case_pairs.is_empty() {
                kind.push_str(" and ");
            }
            kind.push_str(KIND_TYPO);
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
        lines.push(format!(
            "Fix field name {} in debian/upstream/metadata ({}).",
            kind, fixed_str
        ));
    }

    if !prefix_pairs.is_empty() {
        prefix_pairs.sort();
        let fixed_str = prefix_pairs
            .iter()
            .map(|(old, new)| format!("{} ⇒ {}", old, new))
            .collect::<Vec<_>>()
            .join(", ");
        let noun = if prefix_pairs.len() > 1 {
            "fields"
        } else {
            "field"
        };
        lines.push(format!(
            "Prefix unknown {} in debian/upstream/metadata with X- ({}).",
            noun, fixed_str
        ));
    }

    if !removed_fields.is_empty() {
        removed_fields.sort();
        let joined = removed_fields.join(", ");
        let noun = if removed_fields.len() > 1 {
            "fields"
        } else {
            "field"
        };
        lines.push(format!(
            "Remove unknown {} {} from debian/upstream/metadata.",
            noun, joined
        ));
    }

    lines.join("\n")
}

declare_detector! {
    name: "upstream-metadata-field-check",
    tags: ["upstream-metadata-field-unknown"],
    triggers: [debian_workspace::Trigger::UpstreamMetadataField("*")],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(
        base: &Path,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0-1".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        adapter.apply(&ws, preferences)
    }

    fn write_metadata(base: &Path, body: &str) -> std::path::PathBuf {
        let upstream = base.join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();
        let path = upstream.join("metadata");
        fs::write(&path, body).unwrap();
        path
    }

    fn default_prefs() -> FixerPreferences {
        FixerPreferences::default()
    }

    fn opinionated_prefs() -> FixerPreferences {
        FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        }
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
        let path = write_metadata(
            tmp.path(),
            "Name: foo\nRepositry: https://example.org/foo\n",
        );
        let result = run_apply(tmp.path(), &default_prefs()).unwrap();
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
        let path = write_metadata(
            tmp.path(),
            "Name: foo\nX-Repository: https://example.org/foo\n",
        );
        let result = run_apply(tmp.path(), &default_prefs()).unwrap();
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
        let path = write_metadata(
            tmp.path(),
            "Name: foo\nRepository: https://example.org/foo\n",
        );
        let original = fs::read_to_string(&path).unwrap();

        assert!(matches!(
            run_apply(tmp.path(), &default_prefs()),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(tmp.path(), &default_prefs()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_unknown_field_opinionated_prefixes_with_x() {
        // Default opinionated plan is the safe rename to X-<field>,
        // preserving the data. Removal is available as a second plan.
        let tmp = TempDir::new().unwrap();
        let path = write_metadata(
            tmp.path(),
            "Repository: https://example.org/foo\nTotally-Unknown: something\n",
        );
        let result = run_apply(tmp.path(), &opinionated_prefs()).unwrap();
        assert_eq!(
            result.description,
            "Prefix unknown field in debian/upstream/metadata with X- \
             (Totally-Unknown ⇒ X-Totally-Unknown).",
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Repository: https://example.org/foo\nX-Totally-Unknown: something\n",
        );
    }

    #[test]
    fn test_unknown_field_prefix_collision_only_offers_removal() {
        // If `X-<field>` already exists, prefixing would collide, so
        // only removal is on offer. (The X-<field> entry itself is a
        // legitimate extension and gets left alone.)
        let tmp = TempDir::new().unwrap();
        write_metadata(
            tmp.path(),
            "Repository: https://example.org/foo\n\
             Totally-Unknown: a\n\
             X-Totally-Unknown: b\n",
        );
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("test".into()),
            Some("1.0-1".parse().unwrap()),
        );
        let diags = detect(&ws, &default_prefs()).unwrap();
        assert_eq!(diags.len(), 1);
        let plans = &diags[0].plans;
        assert_eq!(plans.len(), 1);
        assert!(plans[0].label.starts_with("unknown-remove\t"));
    }

    #[test]
    fn test_x_prefixed_extension_field_ignored() {
        // Fields in the `X-*` extension namespace are intentional per
        // DEP-12, even if lintian's whitelist still flags them. The
        // fixer leaves them alone.
        let tmp = TempDir::new().unwrap();
        let path = write_metadata(
            tmp.path(),
            "Repository: https://example.org/foo\nX-Custom-Thing: value\n",
        );
        let original = fs::read_to_string(&path).unwrap();
        assert!(matches!(
            run_apply(tmp.path(), &opinionated_prefs()),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_unknown_field_both_plans_offered() {
        // Verify the diagnostic exposes both plans (so an LSP / interactive
        // caller can offer them). The rename comes first by design.
        let tmp = TempDir::new().unwrap();
        write_metadata(
            tmp.path(),
            "Repository: https://example.org/foo\nTotally-Unknown: something\n",
        );
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("test".into()),
            Some("1.0-1".parse().unwrap()),
        );
        let diags = detect(&ws, &default_prefs()).unwrap();
        assert_eq!(diags.len(), 1);
        let plans = &diags[0].plans;
        assert_eq!(plans.len(), 2);
        assert!(plans[0].label.starts_with("unknown-prefix\t"));
        assert!(plans[1].label.starts_with("unknown-remove\t"));
    }

    #[test]
    fn test_unknown_field_non_opinionated_backs_off() {
        let tmp = TempDir::new().unwrap();
        let original = "Repository: https://example.org/foo\nTotally-Unknown: something\n";
        let path = write_metadata(tmp.path(), original);
        assert!(matches!(
            run_apply(tmp.path(), &default_prefs()),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_unknown_certainty_too_low_backs_off() {
        let tmp = TempDir::new().unwrap();
        let original = "Repository: https://example.org/foo\nTotally-Unknown: something\n";
        let path = write_metadata(tmp.path(), original);
        let prefs = FixerPreferences {
            opinionated: Some(true),
            minimum_certainty: Some(Certainty::Likely),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NotCertainEnough(..))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_unknown_field_override_respected() {
        let tmp = TempDir::new().unwrap();
        write_metadata(
            tmp.path(),
            "Repository: https://example.org/foo\nTotally-Unknown: something\n",
        );
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join("lintian-overrides"),
            "upstream-metadata-field-unknown Totally-Unknown [debian/upstream/metadata]\n",
        )
        .unwrap();

        let err = run_apply(tmp.path(), &opinionated_prefs()).unwrap_err();
        assert!(matches!(err, FixerError::NoChangesAfterOverrides(_)));
    }

    #[test]
    fn test_tolerated_name_contact_not_reported() {
        // Name and Contact are listed in DEP12_FIELD_ORDER, so they should
        // not trip the unknown-field branch.
        let tmp = TempDir::new().unwrap();
        write_metadata(
            tmp.path(),
            "Name: Foo\nContact: foo@example.org\nRepository: https://example.org/foo\n",
        );
        assert!(matches!(
            run_apply(tmp.path(), &opinionated_prefs()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_mixed_typo_and_unknown() {
        // Typo renames always fire; the X-prefix rename for unknown
        // fields only fires under opinionated. With opinionated=yes,
        // both happen.
        let tmp = TempDir::new().unwrap();
        let path = write_metadata(
            tmp.path(),
            "Repositry: https://example.org/foo\nTotally-Unknown: something\n",
        );
        let result = run_apply(tmp.path(), &opinionated_prefs()).unwrap();
        assert_eq!(
            result.description,
            "Fix field name typo in debian/upstream/metadata (Repositry ⇒ Repository).\n\
             Prefix unknown field in debian/upstream/metadata with X- \
             (Totally-Unknown ⇒ X-Totally-Unknown).",
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Repository: https://example.org/foo\nX-Totally-Unknown: something\n",
        );
    }
}
