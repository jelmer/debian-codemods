use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use deb822_lossless::Deb822;
use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;

/// Extract license names from a synopsis. Returns a list of licenses, as
/// a list of possible names per license (handles `X with Y exception`).
fn extract_licenses(synopsis: &str) -> Vec<Vec<String>> {
    let mut ret = Vec::new();
    for license in synopsis.split(" or ") {
        let mut options = vec![license.to_string()];
        if let Some((base, _exception)) = license.rsplit_once(" with ") {
            if license.ends_with(" exception") {
                options.push(base.to_string());
            }
        }
        ret.push(options);
    }
    ret
}

fn get_license_name(license_field: &str) -> Option<String> {
    license_field.lines().next().map(|s| s.trim().to_string())
}

fn has_license_text(license_field: &str) -> bool {
    license_field.lines().count() > 1
}

fn collect_defined_licenses(deb822: &Deb822) -> HashSet<String> {
    let mut defined = HashSet::new();
    for paragraph in deb822.paragraphs() {
        let Some(license) = paragraph.get("License") else {
            continue;
        };
        if !has_license_text(&license) {
            continue;
        }
        let Some(name) = get_license_name(&license) else {
            continue;
        };
        defined.insert(name);
    }
    defined
}

fn collect_used_licenses(deb822: &Deb822, defined: &HashSet<String>) -> Vec<Vec<String>> {
    let mut used = Vec::new();
    if let Some(header) = deb822.paragraphs().next() {
        if let Some(license) = header.get("License") {
            if let Some(synopsis) = get_license_name(&license) {
                if defined.contains(&synopsis) {
                    used.push(vec![synopsis.clone()]);
                }
                used.extend(extract_licenses(&synopsis));
            }
        }
    }
    for paragraph in deb822.paragraphs() {
        if paragraph.get("Files").is_none() {
            continue;
        }
        let Some(license) = paragraph.get("License") else {
            continue;
        };
        let Some(synopsis) = get_license_name(&license) else {
            continue;
        };
        if defined.contains(&synopsis) {
            used.push(vec![synopsis.clone()]);
        }
        used.extend(extract_licenses(&synopsis));
    }
    used
}

fn calculate_extra_defined(defined: &HashSet<String>, used: &[Vec<String>]) -> HashSet<String> {
    let mut extra_defined = defined.clone();
    for options in used {
        for option in options {
            extra_defined.remove(option);
        }
    }
    extra_defined
}

fn calculate_extra_used(defined: &HashSet<String>, used: &[Vec<String>]) -> Vec<Vec<String>> {
    let mut extra_used = Vec::new();
    for options in used {
        let found = options.iter().any(|option| defined.contains(option));
        if !found {
            extra_used.push(options.clone());
        }
    }
    extra_used
}

/// Returns `Possible` if any unused license name is referenced in the
/// text of *another* license paragraph or in a Comment field anywhere.
fn check_license_references(deb822: &Deb822, extra_defined: &HashSet<String>) -> Certainty {
    for name in extra_defined {
        for paragraph in deb822.paragraphs() {
            if let Some(license) = paragraph.get("License") {
                let Some(para_name) = get_license_name(&license) else {
                    continue;
                };
                if para_name == *name {
                    continue;
                }
                if license.contains(name) {
                    return Certainty::Possible;
                }
            }
            if let Some(comment) = paragraph.get("Comment") {
                if comment.contains(name) {
                    return Certainty::Possible;
                }
            }
        }
    }
    Certainty::Certain
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let bytes = match ws.read_file(&copyright_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(Vec::new());
    };
    if !content.starts_with("Format:") {
        return Ok(Vec::new());
    }
    let deb822 = match Deb822::from_str(&content) {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()),
    };

    let defined = collect_defined_licenses(&deb822);
    let used = collect_used_licenses(&deb822, &defined);
    let extra_defined = calculate_extra_defined(&defined, &used);
    let extra_used = calculate_extra_used(&defined, &used);

    if extra_defined.is_empty() || !extra_used.is_empty() {
        return Ok(Vec::new());
    }

    let certainty = check_license_references(&deb822, &extra_defined);

    let mut diagnostics = Vec::new();
    for (idx, paragraph) in deb822.paragraphs().enumerate() {
        // Skip the header.
        if idx == 0 {
            continue;
        }
        // Only standalone License paragraphs (no Files: field).
        if paragraph.get("Files").is_some() {
            continue;
        }
        let Some(license) = paragraph.get("License") else {
            continue;
        };
        let Some(name) = get_license_name(&license) else {
            continue;
        };
        if !extra_defined.contains(&name) {
            continue;
        }

        let line_number = paragraph.line() + 1;
        let issue = LintianIssue::source_with_info(
            "unused-license-paragraph-in-dep5-copyright",
            vec![format!("{} [debian/copyright:{}]", name, line_number)],
        );

        // Address by the License field's full value (synopsis + body):
        // unique enough to identify the paragraph. Stash the license
        // name in the message so the describer can list them.
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                format!("name\t{}", name),
                vec![Action::Deb822(Deb822Action::RemoveParagraph {
                    file: copyright_rel.clone(),
                    paragraph: ParagraphSelector::ByKey {
                        field: "License".into(),
                        value: license,
                    },
                })],
            )
            .with_certainty(certainty),
        );
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let names: Vec<&str> = fixed
        .iter()
        .filter_map(|d| d.message.strip_prefix("name\t"))
        .collect();
    format!(
        "Remove unused license definitions for {}.",
        names.join(", ")
    )
}

declare_detector! {
    name: "unused-license-paragraph-in-dep5-copyright",
    tags: ["unused-license-paragraph-in-dep5-copyright"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "License",
            field: "License",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Files",
            field: "License",
        },
    ],
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
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "blah", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_extract_licenses() {
        assert_eq!(extract_licenses("GPL-2+"), vec![vec!["GPL-2+"]]);
        assert_eq!(
            extract_licenses("GPL-2+ or BSD"),
            vec![vec!["GPL-2+"], vec!["BSD"]]
        );
        assert_eq!(
            extract_licenses("GPL-2+ with exception"),
            vec![vec!["GPL-2+ with exception", "GPL-2+"]]
        );
    }

    #[test]
    fn test_remove_unused_license() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: blah\nSource: https://github.com/example/blah\n\nFiles: *\nCopyright: 2013 Somebody <somebody@example.com>\nLicense: GPL-2+\n\nLicense: GPL-2+\n This program is free software; you can redistribute it\n .\n version 2 of the License, or (at your option) any later\n version.\n\nLicense: BSL-1\n Boost Software License, Version 1.0\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove unused license definitions for BSL-1."
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));

        // The removed BSL-1 paragraph leaves a trailing blank line —
        // the lossless deb822 representation tracks the blank-line
        // separator as part of the file rather than the paragraph.
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: blah\nSource: https://github.com/example/blah\n\nFiles: *\nCopyright: 2013 Somebody <somebody@example.com>\nLicense: GPL-2+\n\nLicense: GPL-2+\n This program is free software; you can redistribute it\n .\n version 2 of the License, or (at your option) any later\n version.\n\n",
        );
    }

    #[test]
    fn test_no_changes_when_all_licenses_used() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: 2013 Somebody\nLicense: GPL-2+\n\nLicense: GPL-2+\n This program is free software\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
}
