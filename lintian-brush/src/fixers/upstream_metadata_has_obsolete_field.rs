use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction, YamlAction};
use crate::{FixerError, FixerPreferences};
use debian_workspace::Workspace;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Fields that are only used by addons/tools and shouldn't be in
/// debian/upstream/metadata. If everything else gets dropped, the file
/// itself is redundant.
const ADDON_ONLY_FIELDS: &[&str] = &["Archive"];

/// Extract upstream fields from debian/copyright (Name, Contact).
fn upstream_fields_in_copyright(ws: &dyn Workspace) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let Ok(copyright) = ws.parsed_copyright() else {
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

/// Split a value by separator characters (newlines, multiple spaces, tabs).
fn split_sep_chars(value: &str) -> Vec<String> {
    let sep_regex = Regex::new(r"\n+|\s\s+|\t+").unwrap();
    sep_regex
        .split(value)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let metadata_rel = PathBuf::from("debian/upstream/metadata");
    let yaml_file = match ws.parsed_upstream_metadata() {
        Ok(y) => y,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };
    let Some(doc) = yaml_file.document() else {
        return Ok(Vec::new());
    };
    let Some(mapping) = doc.as_mapping() else {
        return Ok(Vec::new());
    };

    let mut to_remove: Vec<String> = Vec::new();

    // First pass: drop Name/Contact if their value is null or empty.
    for key in ["Name", "Contact"] {
        if !mapping.contains_key(key) {
            continue;
        }
        let value_is_empty_or_null = match mapping.get(key) {
            None => true,
            Some(node) => match node.as_scalar() {
                Some(scalar) => {
                    let s = scalar.value();
                    s.trim().is_empty() || s == "null" || s == "~"
                }
                None => false,
            },
        };
        if value_is_empty_or_null {
            to_remove.push(key.to_string());
        }
    }

    // Second pass: drop Name/Contact if their value matches what's in
    // the machine-readable debian/copyright header.
    let has_name_or_contact = mapping.keys().any(|k| k == "Name" || k == "Contact");
    if has_name_or_contact {
        let copyright_fields = upstream_fields_in_copyright(ws);
        for (field, copyright_value) in &copyright_fields {
            if to_remove.contains(field) {
                continue;
            }
            let Some(node) = mapping.get(field.as_str()) else {
                continue;
            };
            let Some(scalar) = node.as_scalar() else {
                continue;
            };
            let copyright_entries: HashSet<String> = split_sep_chars(copyright_value)
                .iter()
                .map(|s| s.to_lowercase())
                .collect();
            let um_entries: HashSet<String> = split_sep_chars(&scalar.value())
                .iter()
                .map(|s| s.to_lowercase())
                .collect();
            if copyright_entries == um_entries {
                to_remove.push(field.clone());
            }
        }
    }

    if to_remove.is_empty() {
        return Ok(Vec::new());
    }

    // Would the file become empty (or contain only addon-only fields) once
    // these removals are applied? If so, the right action is to delete it
    // outright rather than leave a near-empty file behind.
    let remaining: HashSet<String> = mapping
        .keys()
        .filter_map(|node| match node {
            yaml_edit::YamlNode::Scalar(scalar) => Some(scalar.as_string()),
            _ => None,
        })
        .filter(|k| !to_remove.contains(k))
        .collect();
    let addon_only: HashSet<&'static str> = ADDON_ONLY_FIELDS.iter().copied().collect();
    let only_addon_left = remaining.iter().all(|k| addon_only.contains(k.as_str()));

    let mut sorted_removed = to_remove.clone();
    sorted_removed.sort();
    let description = format!(
        "debian/upstream/metadata has obsolete field{} {} (already in machine-readable debian/copyright).",
        if sorted_removed.len() > 1 { "s" } else { "" },
        sorted_removed.join(", ")
    );
    let label = format!(
        "Remove obsolete field{} {} from debian/upstream/metadata (already present in machine-readable debian/copyright).",
        if sorted_removed.len() > 1 { "s" } else { "" },
        sorted_removed.join(", ")
    );

    let actions: Vec<Action> = if only_addon_left {
        // Replace the per-field rewrites with a Delete + best-effort
        // RemoveDirIfEmpty on the parent: the file doesn't carry useful
        // information any more, and debian/upstream/ is typically empty
        // once the metadata file is gone.
        vec![
            Action::Filesystem(FilesystemAction::Delete {
                file: metadata_rel.clone(),
            }),
            Action::Filesystem(FilesystemAction::RemoveDirIfEmpty {
                file: PathBuf::from("debian/upstream"),
            }),
        ]
    } else {
        to_remove
            .into_iter()
            .map(|key| {
                Action::Yaml(YamlAction::RemoveField {
                    file: metadata_rel.clone(),
                    parent_path: Vec::new(),
                    key,
                })
            })
            .collect()
    };

    Ok(vec![Diagnostic::untagged(description, label, actions)])
}

declare_detector! {
    name: "upstream-metadata-has-obsolete-field",
    tags: [],
    triggers: [
        debian_workspace::Trigger::UpstreamMetadataField("Name"),
        debian_workspace::Trigger::UpstreamMetadataField("Contact"),
        debian_workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Format",
            field: "Upstream-Name",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Format",
            field: "Upstream-Contact",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_no_metadata_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_remove_null_field() {
        let tmp = TempDir::new().unwrap();
        let upstream = tmp.path().join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();
        let path = upstream.join("metadata");
        fs::write(&path, "Name: test-package\nContact: null\n").unwrap();

        run_apply(tmp.path()).unwrap();

        // Contact dropped; Name kept.
        assert_eq!(fs::read_to_string(&path).unwrap(), "Name: test-package\n");
    }

    #[test]
    fn test_remove_obsolete_field_from_copyright() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        let upstream = debian.join("upstream");
        fs::create_dir_all(&upstream).unwrap();

        fs::write(
            debian.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test-package\nUpstream-Contact: Test User <test@example.com>\n\nFiles: *\nCopyright: 2024 Test User <test@example.com>\nLicense: GPL-3+\n\nLicense: GPL-3+\n This program is free software.\n",
        )
        .unwrap();

        let metadata_path = upstream.join("metadata");
        fs::write(
            &metadata_path,
            "Name: test-package\nContact: Test User <test@example.com>\nRepository: https://github.com/example/test\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&metadata_path).unwrap(),
            "Repository: https://github.com/example/test\n",
        );
    }

    #[test]
    fn test_file_is_deleted_when_only_addon_fields_remain() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        let upstream = debian.join("upstream");
        fs::create_dir_all(&upstream).unwrap();

        let metadata_path = upstream.join("metadata");
        // Name and Contact will be dropped (null); only Archive remains,
        // which is an addon-only field, so the file should go away.
        fs::write(
            &metadata_path,
            "Name: null\nContact: null\nArchive: SourceForge\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert!(!metadata_path.exists());
    }

    #[test]
    fn test_no_change_when_nothing_obsolete() {
        let tmp = TempDir::new().unwrap();
        let upstream = tmp.path().join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();
        let path = upstream.join("metadata");
        let original = "Name: test-package\nRepository: https://github.com/example/test\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
}
