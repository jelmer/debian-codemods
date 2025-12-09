use crate::{declare_fixer, Certainty, FixerError, FixerResult, LintianIssue};
use deb822_lossless::Deb822;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::str::FromStr;

/// Extract license names from a synopsis.
///
/// This will return a list of licenses, as a list of possible names per license.
fn extract_licenses(synopsis: &str) -> Vec<Vec<String>> {
    let mut ret = Vec::new();
    for license in synopsis.split(" or ") {
        let mut options = vec![license.to_string()];
        // Handle "license with exception" pattern
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

    // Collect from header
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

    // Collect from Files paragraphs
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

fn remove_unused_license_paragraphs(
    deb822: &mut Deb822,
    extra_defined: &HashSet<String>,
) -> Vec<(String, usize)> {
    let mut indices_to_remove = Vec::new();
    let mut removed_licenses = Vec::new();

    for (idx, paragraph) in deb822.paragraphs().enumerate() {
        // Skip header (first paragraph)
        if idx == 0 {
            continue;
        }

        // Check if this is a standalone License paragraph (not Files paragraph)
        if paragraph.get("Files").is_some() {
            continue;
        }
        let Some(license) = paragraph.get("License") else {
            continue;
        };
        let Some(name) = get_license_name(&license) else {
            continue;
        };
        if extra_defined.contains(&name) {
            let line_number = paragraph.line() + 1;
            indices_to_remove.push(idx);
            removed_licenses.push((name.clone(), line_number));
        }
    }

    // Remove in reverse order to maintain indices
    for idx in indices_to_remove.iter().rev() {
        deb822.remove_paragraph(*idx);
    }

    removed_licenses
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");

    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&copyright_path)?;

    if !content.starts_with("Format:") {
        return Err(FixerError::NoChanges);
    }

    let mut deb822 = Deb822::from_str(&content).map_err(|_| FixerError::NoChanges)?;

    let defined = collect_defined_licenses(&deb822);
    let used = collect_used_licenses(&deb822, &defined);
    let extra_defined = calculate_extra_defined(&defined, &used);
    let extra_used = calculate_extra_used(&defined, &used);

    // Only proceed if we have unused definitions and no missing definitions
    if extra_defined.is_empty() || !extra_used.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let mut certainty = Certainty::Certain;

    // If there are undefined licenses, drop certainty
    if !extra_used.is_empty() {
        certainty = Certainty::Possible;
    }

    // Check if unused licenses are referenced in text or comments
    let reference_certainty = check_license_references(&deb822, &extra_defined);
    if reference_certainty == Certainty::Possible {
        certainty = Certainty::Possible;
    }

    let removed_licenses = remove_unused_license_paragraphs(&mut deb822, &extra_defined);

    let new_content = deb822.to_string();
    if new_content == content {
        return Err(FixerError::NoChanges);
    }

    fs::write(&copyright_path, new_content)?;

    // Create LintianIssue for each removed license
    let mut fixed_issues = Vec::new();
    for (license_name, line_number) in &removed_licenses {
        let issue = LintianIssue::source_with_info(
            "unused-license-paragraph-in-dep5-copyright",
            vec![format!(
                "{} [debian/copyright:{}]",
                license_name, line_number
            )],
        );
        fixed_issues.push(issue);
    }

    let license_list: Vec<_> = extra_defined.iter().cloned().collect();
    Ok(FixerResult::builder(format!(
        "Remove unused license definitions for {}",
        license_list.join(", ")
    ))
    .certainty(certainty)
    .fixed_issues(fixed_issues)
    .build())
}

declare_fixer! {
    name: "unused-license-paragraph-in-dep5-copyright",
    tags: ["unused-license-paragraph-in-dep5-copyright"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_extract_licenses() {
        let licenses = extract_licenses("GPL-2+");
        assert_eq!(licenses, vec![vec!["GPL-2+"]]);

        let licenses = extract_licenses("GPL-2+ or BSD");
        assert_eq!(licenses, vec![vec!["GPL-2+"], vec!["BSD"]]);

        let licenses = extract_licenses("GPL-2+ with exception");
        assert_eq!(licenses, vec![vec!["GPL-2+ with exception", "GPL-2+"]]);
    }

    #[test]
    fn test_remove_unused_license() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let copyright_content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: blah
Source: https://github.com/example/blah

Files: *
Copyright: 2013 Somebody <somebody@example.com>
License: GPL-2+

License: GPL-2+
 This program is free software; you can redistribute it
 and/or modify it under the terms of the GNU General Public
 License as published by the Free Software Foundation; either
 version 2 of the License, or (at your option) any later
 version.

License: BSL-1
 Boost Software License, Version 1.0
"#;

        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, copyright_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "blah", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&copyright_path).unwrap();
        assert!(!updated_content.contains("BSL-1"));
        assert!(updated_content.contains("GPL-2+"));
    }

    #[test]
    fn test_no_changes_when_all_licenses_used() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let copyright_content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2013 Somebody
License: GPL-2+

License: GPL-2+
 This program is free software
"#;

        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, copyright_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "blah", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
