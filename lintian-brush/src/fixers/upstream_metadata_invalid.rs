use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

const SEQUENCE_FIELDS: &[&str] = &["Reference", "Screenshots"];

/// Result of attempting to fix duplicate keys in-memory.
struct DedupResult {
    /// New file content if any duplicates were merged. `None` means
    /// nothing to fix.
    new_content: Option<String>,
    /// Field names that had duplicates, one entry per dropped duplicate
    /// (e.g. `["Reference", "Reference"]` if there were three copies).
    duplicates: Vec<String>,
}

/// Fix duplicate keys: sequence fields get merged into a list, others
/// keep the first value. Operates in-memory. Returns `NoChange` if the
/// file is multi-document (handled by [`drop_empty_documents`] later).
fn dedup_keys(content: &str) -> Result<DedupResult, FixerError> {
    let doc = match yaml_edit::Document::from_str(content) {
        Ok(d) => d,
        Err(_) => {
            return Ok(DedupResult {
                new_content: None,
                duplicates: Vec::new(),
            });
        }
    };
    let Some(mapping) = doc.as_mapping() else {
        return Ok(DedupResult {
            new_content: None,
            duplicates: Vec::new(),
        });
    };

    let mut key_values: HashMap<String, Vec<yaml_edit::YamlNode>> = HashMap::new();
    for (key, value) in mapping.iter() {
        if let yaml_edit::YamlNode::Scalar(key_scalar) = key {
            let key_str = key_scalar.as_string();
            key_values.entry(key_str).or_default().push(value);
        }
    }

    let duplicate_keys: Vec<String> = key_values
        .iter()
        .filter(|(_, values)| values.len() > 1)
        .map(|(key, _)| key.clone())
        .collect();
    if duplicate_keys.is_empty() {
        return Ok(DedupResult {
            new_content: None,
            duplicates: Vec::new(),
        });
    }

    let mut duplicates = Vec::new();
    for key in &duplicate_keys {
        let values = &key_values[key];
        let is_sequence_field = SEQUENCE_FIELDS.contains(&key.as_str());

        if is_sequence_field {
            while mapping.remove(key.as_str()).is_some() {}
            let mut seq_builder = yaml_edit::YamlBuilder::sequence();
            for value in values {
                seq_builder = seq_builder.item(value);
            }
            let yaml_builder = seq_builder.build();
            let seq_file = yaml_builder.build();
            if let Some(seq_doc) = seq_file.documents().next() {
                if let Some(seq) = seq_doc.as_sequence() {
                    mapping.set(key.as_str(), seq);
                }
            }
        } else {
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

        for _ in 0..(values.len() - 1) {
            duplicates.push(key.clone());
        }
    }

    Ok(DedupResult {
        new_content: Some(doc.to_string()),
        duplicates,
    })
}

/// If the top-level node is a sequence, return the rewritten content and
/// a count of the original list items (one issue per item).
fn unwrap_top_level_sequence(content: &str) -> Result<Option<(String, usize)>, FixerError> {
    let doc = match yaml_edit::Document::from_str(content) {
        Ok(d) => d,
        Err(_) => return Ok(None),
    };
    let Some(sequence) = doc.as_sequence() else {
        return Ok(None);
    };
    let items: Vec<_> = sequence.values().collect();

    if items.len() == 1 {
        let item_text = items[0].to_string().trim().to_string();
        return Ok(Some((item_text, 1)));
    }

    let all_single_key_mappings = items.iter().all(|item| {
        if let yaml_edit::YamlNode::Mapping(mapping_node) = item {
            mapping_node.entries().count() == 1
        } else {
            false
        }
    });
    if !all_single_key_mappings {
        return Ok(None);
    }

    let count = items.len();
    let new_mapping = yaml_edit::Mapping::new();
    let new_doc = yaml_edit::Document::from_mapping(new_mapping);
    let doc_mapping = new_doc.as_mapping().unwrap();
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
    Ok(Some((doc_mapping.to_string(), count)))
}

/// Outcome of dropping empty documents.
enum EmptyDocsOutcome {
    /// No empty documents found.
    NoChange,
    /// All documents were empty — file should be deleted.
    DeleteFile,
    /// Rewrite the file to keep only the first non-empty document.
    Rewrite(String),
}

fn drop_empty_documents(original: &str) -> Result<EmptyDocsOutcome, FixerError> {
    let yaml = yaml_edit::YamlFile::from_str(original)
        .map_err(|e| FixerError::Other(format!("Failed to parse YAML: {}", e)))?;
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
            let s = scalar.as_string();
            if s.trim().is_empty() || s.trim().starts_with("%YAML") {
                has_empty = true;
                break;
            }
        } else {
            has_empty = true;
            break;
        }
    }
    if !has_empty {
        return Ok(EmptyDocsOutcome::NoChange);
    }

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
                false
            }
        })
        .collect();

    if non_empty_docs.is_empty() {
        return Ok(EmptyDocsOutcome::DeleteFile);
    }

    let leading_content = if let Some(pos) = original.find("---") {
        &original[..pos]
    } else {
        ""
    };
    let doc_content = non_empty_docs[0].to_string();
    let final_content = if !leading_content.trim().is_empty() {
        format!("{}{}", leading_content, doc_content)
    } else {
        doc_content
    };
    Ok(EmptyDocsOutcome::Rewrite(final_content))
}

/// Per-diagnostic action selector — the framework needs each diagnostic
/// to carry an action plan, but multiple diagnostics here describe the
/// same single rewrite. We therefore route them all to the same action,
/// which is just the file write.
pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let metadata_rel = PathBuf::from("debian/upstream/metadata");
    let bytes = match ws.read_file(&metadata_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let original = String::from_utf8(bytes).map_err(|e| {
        FixerError::Other(format!(
            "debian/upstream/metadata is not valid UTF-8: {}",
            e
        ))
    })?;
    let mut current = original.clone();
    let mut delete_file = false;

    let mut yaml_invalid_count = 0usize;
    let mut yaml_not_mapping_count = 0usize;
    let mut empty_docs_descs: Vec<&'static str> = Vec::new();
    let mut dedup_fields: Vec<String> = Vec::new();

    // 1. Deduplicate keys.
    let dedup = dedup_keys(&current)?;
    if let Some(new_content) = dedup.new_content {
        current = new_content;
        yaml_invalid_count = 1;
        dedup_fields = dedup.duplicates;
    }

    // 2. Unwrap top-level sequence into a mapping.
    if let Some((new_content, count)) = unwrap_top_level_sequence(&current)? {
        current = new_content;
        yaml_not_mapping_count = count;
    }

    // 3. Drop empty documents.
    match drop_empty_documents(&current)? {
        EmptyDocsOutcome::NoChange => {}
        EmptyDocsOutcome::DeleteFile => {
            delete_file = true;
            empty_docs_descs.push("Remove empty debian/upstream/metadata file.");
        }
        EmptyDocsOutcome::Rewrite(new_content) => {
            current = new_content;
            empty_docs_descs
                .push("Discard extra empty YAML documents in debian/upstream/metadata.");
        }
    }

    if !delete_file && current == original {
        return Ok(Vec::new());
    }

    // Build the single action this fixer applies.
    let action = if delete_file {
        Action::Filesystem(FilesystemAction::Delete {
            file: metadata_rel.clone(),
        })
    } else {
        Action::Filesystem(FilesystemAction::Write {
            file: metadata_rel.clone(),
            content: current.into_bytes(),
        })
    };

    // Build a diagnostic per surviving issue. They all carry the same
    // Write/Delete action; the applier deduplicates by value-equality,
    // so duplicates are no-ops, and the action survives as long as any
    // one diagnostic survives override filtering.
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    if yaml_invalid_count > 0 {
        let mut sorted_fields = dedup_fields;
        sorted_fields.sort();
        let desc = format!(
            "Remove duplicate values for fields {} in debian/upstream/metadata.",
            sorted_fields.join(", ")
        );
        diagnostics.push(Diagnostic::with_actions(
            LintianIssue::source("upstream-metadata-yaml-invalid"),
            desc,
            vec![action.clone()],
        ));
    }
    for _ in 0..yaml_not_mapping_count {
        diagnostics.push(Diagnostic::with_actions(
            LintianIssue::source("upstream-metadata-not-yaml-mapping"),
            "Use YAML mapping in debian/upstream/metadata.",
            vec![action.clone()],
        ));
    }
    for desc in empty_docs_descs {
        diagnostics.push(Diagnostic::untagged(desc.to_string(), vec![action.clone()]));
    }

    if diagnostics.is_empty() {
        // The change isn't motivated by a specific lintian issue — emit
        // a generic untagged diagnostic so the action still runs.
        diagnostics.push(Diagnostic::untagged(
            "Fix invalid debian/upstream/metadata.".to_string(),
            vec![action],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut parts: Vec<String> = Vec::new();
    for d in fixed {
        if seen.insert(d.message.clone()) {
            parts.push(d.message.clone());
        }
    }
    parts.join(" ")
}

declare_detector! {
    name: "upstream-metadata-invalid",
    tags: [],
    triggers: [crate::workspace::Trigger::File("debian/upstream/metadata")],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_no_metadata_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_dedup_scalar_field() {
        let tmp = TempDir::new().unwrap();
        let upstream = tmp.path().join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();
        let path = upstream.join("metadata");
        fs::write(&path, "Name: foo\nName: bar\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "Name: foo\n");
    }

    #[test]
    fn test_unwrap_single_element_sequence() {
        let tmp = TempDir::new().unwrap();
        let upstream = tmp.path().join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();
        let path = upstream.join("metadata");
        fs::write(&path, "- Name: foo\n  Bug-Database: https://example.com\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Name: foo\n  Bug-Database: https://example.com",
        );
    }
}
