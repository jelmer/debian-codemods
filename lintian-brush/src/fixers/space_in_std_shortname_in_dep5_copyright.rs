use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_copyright::lossless::Copyright;
use debian_copyright::License;
use lazy_static::lazy_static;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

// Include the generated SPDX license data
include!(concat!(env!("OUT_DIR"), "/spdx_licenses.rs"));

lazy_static! {
    static ref RENAMES_MAP: indexmap::IndexMap<String, String> = {
        // Start with SPDX license name to ID mapping
        let mut map = get_spdx_license_renames()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<indexmap::IndexMap<_, _>>();

        // Add the hardcoded renames from the Python version (RENAMES dict)
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

        // Add the hardcoded values from the Python version (REPLACE_SPACES set)
        set.insert("public-domain".to_string());
        set.insert("mit-style".to_string());
        set.insert("bsd-style".to_string());

        // Add all SPDX license IDs (lowercased)
        for license_id in SPDX_LICENSE_IDS {
            set.insert(license_id.to_lowercase());
            // Also add versions without trailing .0
            if license_id.ends_with(".0") {
                set.insert(license_id[..license_id.len() - 2].to_lowercase());
            }
        }

        set
    };
}

/// Fix spaces in a license synopsis
fn fix_spaces_in_synopsis(synopsis: &str) -> Option<String> {
    if !synopsis.contains(' ') {
        return None;
    }

    // Split by " or " or " | "
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

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");

    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&copyright_path)?;
    let copyright: Copyright = content.parse().map_err(|_| FixerError::NoChanges)?;

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Fix Files paragraphs
    for mut files_para in copyright.iter_files() {
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
        let issue = LintianIssue::source_with_info(
            "space-in-std-shortname-in-dep5-copyright",
            vec![format!(
                "{} [debian/copyright:{}]",
                name.to_lowercase(),
                line_number
            )],
        );

        if issue.should_fix(base_path) {
            let new_license = if let Some(text) = license.text() {
                License::Named(new_synopsis.clone(), text.to_string())
            } else {
                License::Name(new_synopsis.clone())
            };
            files_para.set_license(&new_license);
            fixed_issues.push(issue);
        } else {
            overridden_issues.push(issue);
        }
    }

    // Fix License paragraphs
    for mut license_para in copyright.iter_licenses() {
        let Some(name) = license_para.name() else {
            continue;
        };
        let Some(new_synopsis) = fix_spaces_in_synopsis(&name) else {
            continue;
        };

        let line_number = license_para.as_deb822().line() + 1;
        let issue = LintianIssue::source_with_info(
            "space-in-std-shortname-in-dep5-copyright",
            vec![format!(
                "{} [debian/copyright:{}]",
                name.to_lowercase(),
                line_number
            )],
        );

        if issue.should_fix(base_path) {
            let new_license = if let Some(text) = license_para.text() {
                License::Named(new_synopsis.clone(), text)
            } else {
                License::Name(new_synopsis.clone())
            };
            license_para.set_license(&new_license);
            fixed_issues.push(issue);
        } else {
            overridden_issues.push(issue);
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    fs::write(&copyright_path, copyright.to_string())?;

    Ok(
        FixerResult::builder("Replace spaces in short license names with dashes.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "space-in-std-shortname-in-dep5-copyright",
    tags: ["space-in-std-shortname-in-dep5-copyright"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
