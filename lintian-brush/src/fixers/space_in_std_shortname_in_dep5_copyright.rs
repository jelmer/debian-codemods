use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use lazy_static::lazy_static;
use std::collections::HashSet;
use std::path::PathBuf;

// Include the generated SPDX license data
include!(concat!(env!("OUT_DIR"), "/spdx_licenses.rs"));

lazy_static! {
    static ref RENAMES_MAP: indexmap::IndexMap<String, String> = {
        let mut map = get_spdx_license_renames()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<indexmap::IndexMap<_, _>>();

        map.insert(
            "creative commons attribution share-alike (cc-by-sa) v3.0".to_string(),
            "CC-BY-SA-3.0".to_string(),
        );
        map.insert(
            "apache license version 2.0".to_string(),
            "Apache-2.0".to_string(),
        );

        map
    };
    static ref REPLACE_SPACES_SET: HashSet<String> = {
        let mut set = HashSet::new();
        set.insert("public-domain".to_string());
        set.insert("mit-style".to_string());
        set.insert("bsd-style".to_string());
        for license_id in SPDX_LICENSE_IDS {
            set.insert(license_id.to_lowercase());
            if let Some(without_suffix) = license_id.strip_suffix(".0") {
                set.insert(without_suffix.to_lowercase());
            }
        }
        set
    };
}

/// Fix spaces in a license synopsis. Returns the rewritten synopsis if a
/// change is needed, `None` if the synopsis is already in canonical form.
fn fix_spaces_in_synopsis(synopsis: &str) -> Option<String> {
    if !synopsis.contains(' ') {
        return None;
    }
    let ors = synopsis
        .replace(" | ", " or ")
        .split(" or ")
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let mut names = Vec::new();
    let mut changed = false;
    for name in ors {
        let new_name = if let Some(renamed) = RENAMES_MAP.get(&name.to_lowercase()) {
            changed = true;
            renamed.clone()
        } else {
            let name_with_dashes = name.replace(' ', "-");
            if REPLACE_SPACES_SET.contains(&name_with_dashes.to_lowercase()) {
                changed = true;
                name_with_dashes
            } else {
                name
            }
        };
        names.push(new_name);
    }
    if changed {
        Some(names.join(" or "))
    } else {
        None
    }
}

/// Build the new License field value from a possibly-multiline existing
/// value and a rewritten synopsis. The synopsis is the first line; any
/// continuation lines are preserved verbatim.
fn rewrite_license_value(existing: &str, new_synopsis: &str) -> String {
    let mut lines = existing.split('\n');
    let _old_first = lines.next().unwrap_or("");
    let rest: Vec<&str> = lines.collect();
    if rest.is_empty() {
        new_synopsis.to_string()
    } else {
        format!("{}\n{}", new_synopsis, rest.join("\n"))
    }
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let copyright = match ws.parsed_copyright() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };

    let mut diagnostics = Vec::new();

    for files_para in copyright.iter_files() {
        let Some(license) = files_para.license() else {
            continue;
        };
        let Some(name) = license.name() else {
            continue;
        };
        let Some(new_synopsis) = fix_spaces_in_synopsis(name) else {
            continue;
        };
        let line_number = files_para.as_deb822().line() + 1;
        // Address the paragraph by its Files: field value.
        let files_value = files_para.as_deb822().get("Files").unwrap_or_default();
        let raw_license = files_para.as_deb822().get("License").unwrap_or_default();
        let new_value = rewrite_license_value(&raw_license, &new_synopsis);

        let issue = LintianIssue::source_with_info(
            "space-in-std-shortname-in-dep5-copyright",
            Visibility::Warning,
            vec![format!(
                "{} [debian/copyright:{}]",
                name.to_lowercase(),
                line_number
            )],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "License short name in debian/copyright contains a space.",
            "Replace spaces in short license names with dashes.",
            vec![Action::Deb822(Deb822Action::SetField {
                file: copyright_rel.clone(),
                paragraph: ParagraphSelector::CopyrightFiles { glob: files_value },
                field: "License".into(),
                value: new_value,
            })],
        ));
    }

    for license_para in copyright.iter_licenses() {
        let Some(name) = license_para.name() else {
            continue;
        };
        let Some(new_synopsis) = fix_spaces_in_synopsis(&name) else {
            continue;
        };
        let line_number = license_para.as_deb822().line() + 1;
        let raw_license = license_para.as_deb822().get("License").unwrap_or_default();
        let new_value = rewrite_license_value(&raw_license, &new_synopsis);

        let issue = LintianIssue::source_with_info(
            "space-in-std-shortname-in-dep5-copyright",
            Visibility::Warning,
            vec![format!(
                "{} [debian/copyright:{}]",
                name.to_lowercase(),
                line_number
            )],
        );

        // License paragraphs (no Files: field) are addressed by their
        // License field's full value — that's the synopsis plus any
        // continuation lines, which uniquely identifies the paragraph
        // before the rewrite.
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "License short name in debian/copyright contains a space.",
            "Replace spaces in short license names with dashes.",
            vec![Action::Deb822(Deb822Action::SetField {
                file: copyright_rel.clone(),
                paragraph: ParagraphSelector::ByKey {
                    field: "License".into(),
                    value: raw_license,
                },
                field: "License".into(),
                value: new_value,
            })],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "space-in-std-shortname-in-dep5-copyright",
    tags: ["space-in-std-shortname-in-dep5-copyright"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Files",
            field: "License",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "License",
            field: "License",
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
    fn test_fix_spaces_in_synopsis_no_spaces() {
        assert_eq!(fix_spaces_in_synopsis("Apache-2.0"), None);
    }

    #[test]
    fn test_fix_spaces_in_synopsis_known_rename() {
        assert_eq!(
            fix_spaces_in_synopsis("Creative Commons Attribution Share-Alike (CC-BY-SA) v3.0"),
            Some("CC-BY-SA-3.0".to_string())
        );
    }

    #[test]
    fn test_fix_spaces_in_synopsis_replace_spaces() {
        assert_eq!(
            fix_spaces_in_synopsis("Apache 2.0"),
            Some("Apache-2.0".to_string())
        );
        assert_eq!(fix_spaces_in_synopsis("GPL 3"), Some("GPL-3".to_string()));
    }

    #[test]
    fn test_fix_spaces_in_synopsis_with_or() {
        assert_eq!(
            fix_spaces_in_synopsis("Apache 2.0 | GPL 3"),
            Some("Apache-2.0 or GPL-3".to_string())
        );
    }

    #[test]
    fn test_files_paragraph_license_synopsis_rewrite() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: foo\n\nFiles: *\nCopyright: 2024 Foo\nLicense: Apache 2.0\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: foo\n\nFiles: *\nCopyright: 2024 Foo\nLicense: Apache-2.0\n",
        );
    }

    #[test]
    fn test_no_change_when_already_canonical() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: foo\n\nFiles: *\nCopyright: 2024 Foo\nLicense: Apache-2.0\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
