use crate::licenses::{COMMON_LICENSES_DIR, FULL_LICENSE_NAME};
use crate::{FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_copyright::lossless::Copyright;
use debian_copyright::License;
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

lazy_static! {
    static ref SPDX_RENAMES: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert("BSD", "BSD-3-clause");
        m
    };
    static ref CANONICAL_NAMES: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert("CC0", "CC0-1.0");
        m
    };
    static ref BLURBS: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert(
            "CC0-1.0",
            "\
To the extent possible under law, the author(s) have dedicated all copyright
and related and neighboring rights to this software to the public domain
worldwide. This software is distributed without any warranty.

You should have received a copy of the CC0 Public Domain Dedication along with
this software. If not, see <http://creativecommons.org/publicdomain/zero/1.0/>.",
        );

        m.insert(
            "Apache-2.0",
            "\
Licensed under the Apache License, Version 2.0 (the \"License\");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

     http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an \"AS IS\" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.",
        );

        m.insert(
            "GPL-2+",
            "\
This package is free software; you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation; either version 2 of the License, or
(at your option) any later version.

This package is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program. If not, see <http://www.gnu.org/licenses/>",
        );

        m.insert(
            "GPL-3+",
            "\
This package is free software; you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation; either version 3 of the License, or
(at your option) any later version.

This package is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program. If not, see <http://www.gnu.org/licenses/>",
        );

        m
    };
    static ref WHITESPACE_RE: Regex = Regex::new(r"[\n\t ]+").unwrap();
    static ref CANONICAL_RE: Regex = Regex::new(r"^([A-Za-z0-9]+)(-[0-9\.]+)?(\+)?$").unwrap();
}

fn normalize_license_text(text: &str) -> String {
    WHITESPACE_RE.replace_all(text.trim(), " ").to_string()
}

fn load_common_license(name: &str) -> Option<String> {
    let path = Path::new(COMMON_LICENSES_DIR).join(name);
    fs::read_to_string(path)
        .ok()
        .map(|text| normalize_license_text(&text))
}

fn load_common_licenses() -> Vec<(String, String)> {
    let mut licenses = Vec::new();

    // Special handling for CC0-1.0
    if let Some(text) = load_common_license("CC0-1.0") {
        // Remove "Legal Code " from CC0 text
        let text = text.replace("Legal Code ", "");
        licenses.push(("CC0-1.0".to_string(), text));
    }

    // Load other common licenses
    if let Ok(entries) = fs::read_dir(COMMON_LICENSES_DIR) {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                if name == "CC0-1.0" {
                    continue; // Already handled
                }
                if let Some(text) = load_common_license(&name) {
                    let spdx_name = SPDX_RENAMES
                        .get(name.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| name.clone());
                    licenses.push((spdx_name, text));
                }
            }
        }
    }

    licenses
}

fn drop_debian_file_reference(text: &str) -> Option<String> {
    text.to_lowercase()
        .find("on debian systems, ")
        .map(|pos| text[..pos].trim().to_string())
}

fn debian_file_reference(name: &str, filename: &str) -> String {
    let text = format!(
        "On Debian systems, the full text of the {} can be found in the file `/usr/share/common-licenses/{}'.",
        name, filename
    );

    // Wrap to 78 characters
    textwrap::fill(&text, 78)
}

fn find_common_license_from_fulltext(text: &str) -> Option<String> {
    // Don't bother for anything that's short
    if text.lines().count() < 15 {
        return None;
    }

    let normalized = normalize_license_text(text);
    let normalized_without_ref =
        drop_debian_file_reference(&normalized).unwrap_or_else(|| normalized.clone());

    let common_licenses = load_common_licenses();
    for (shortname, fulltext) in &common_licenses {
        if fulltext == &normalized || fulltext == &normalized_without_ref {
            return Some(shortname.clone());
        }
    }

    None
}

fn find_common_license_from_blurb(text: &str) -> Option<String> {
    let normalized = normalize_license_text(text);
    let normalized_without_ref = drop_debian_file_reference(&normalized);

    for (name, blurb) in BLURBS.iter() {
        let normalized_blurb = normalize_license_text(blurb);
        if normalized == normalized_blurb {
            return Some(name.to_string());
        }
        if let Some(ref text_without_ref) = normalized_without_ref {
            if text_without_ref == &normalized_blurb {
                return Some(name.to_string());
            }
        }
    }

    None
}

fn canonical_license_id(license_id: &str) -> String {
    if let Some(caps) = CANONICAL_RE.captures(license_id) {
        let family = caps.get(1).unwrap().as_str();
        let mut version = caps
            .get(2)
            .map(|m| &m.as_str()[1..])
            .unwrap_or("1")
            .to_string();
        let plus = caps.get(3).map(|m| m.as_str()).unwrap_or("");

        // Remove trailing .0
        while version.ends_with(".0") {
            version = version[..version.len() - 2].to_string();
        }

        format!("{}-{}{}", family, version, plus)
    } else {
        tracing::warn!("Unable to get canonical name for {:?}", license_id);
        license_id.to_string()
    }
}

pub fn run(base_path: &Path, _preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");
    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&copyright_path)?;
    let (copyright, _errors) = match Copyright::from_str_relaxed(&content) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("debian/copyright is not machine-readable: {:?}", e);
            return Err(FixerError::NoChanges);
        }
    };

    let mut updated = HashSet::new();
    let mut renames: HashMap<String, String> = HashMap::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Process all license paragraphs and apply changes directly
    for mut para in copyright.iter_licenses() {
        let Some(synopsis) = para.name() else {
            continue;
        };
        let Some(text) = para.text() else {
            continue;
        };

        if text.is_empty() {
            continue;
        }

        let mut replaced_with_blurb = false;

        // Try to replace full license text with blurb
        if let Some(license_matched) = find_common_license_from_fulltext(&text) {
            let canonical_id = canonical_license_id(&synopsis);

            // Find matching blurb
            let mut found_blurb = None;
            for (shortname, blurb) in BLURBS.iter() {
                if canonical_id == canonical_license_id(shortname) {
                    found_blurb = Some((shortname, blurb));
                    break;
                }
            }

            if let Some((_shortname, blurb)) = found_blurb {
                // Check which tags this would fix and if they should be fixed
                let mut should_apply = false;

                // Check full license tags
                if license_matched == "Apache-2.0" {
                    let issue = LintianIssue::source_with_info(
                        "copyright-file-contains-full-apache-2-license",
                        vec![license_matched.clone()],
                    );
                    if issue.should_fix(base_path) {
                        fixed_issues.push(issue);
                        should_apply = true;
                    } else {
                        overridden_issues.push(issue);
                    }
                }
                if license_matched.starts_with("GFDL-") {
                    let issue = LintianIssue::source_with_info(
                        "copyright-file-contains-full-gfdl-license",
                        vec![license_matched.clone()],
                    );
                    if issue.should_fix(base_path) {
                        fixed_issues.push(issue);
                        should_apply = true;
                    } else {
                        overridden_issues.push(issue);
                    }
                }
                if license_matched.starts_with("GPL-") {
                    let issue = LintianIssue::source_with_info(
                        "copyright-file-contains-full-gpl-license",
                        vec![license_matched.clone()],
                    );
                    if issue.should_fix(base_path) {
                        fixed_issues.push(issue);
                        should_apply = true;
                    } else {
                        overridden_issues.push(issue);
                    }
                }

                // Check common license reference tags
                let common_ref_issue = LintianIssue::source_with_info(
                    "copyright-does-not-refer-to-common-license-file",
                    vec![license_matched.clone()],
                );
                if common_ref_issue.should_fix(base_path) {
                    fixed_issues.push(common_ref_issue);
                    should_apply = true;
                } else {
                    overridden_issues.push(common_ref_issue);
                }

                // Check license-specific common license tags
                let specific_tag = if license_matched.starts_with("Apache-2") {
                    Some("copyright-not-using-common-license-for-apache2")
                } else if license_matched.starts_with("GPL-") {
                    Some("copyright-not-using-common-license-for-gpl")
                } else if license_matched.starts_with("LGPL-") {
                    Some("copyright-not-using-common-license-for-lgpl")
                } else if license_matched.starts_with("GFDL-") {
                    Some("copyright-not-using-common-license-for-gfdl")
                } else {
                    None
                };

                if let Some(tag) = specific_tag {
                    let issue = LintianIssue::source_with_info(tag, vec![license_matched.clone()]);
                    if issue.should_fix(base_path) {
                        fixed_issues.push(issue);
                        should_apply = true;
                    } else {
                        overridden_issues.push(issue);
                    }
                }

                // Only apply the change if at least one issue should be fixed
                if should_apply {
                    // Apply the change directly - set blurb with reference
                    let license_name = FULL_LICENSE_NAME
                        .get(license_matched.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| license_matched.clone());
                    let reference = debian_file_reference(&license_name, &license_matched);
                    let text_with_reference = format!("{}\n\n{}", blurb, reference);

                    // Use the matched license name (which might be canonical, like CC0-1.0)
                    para.set_license(&License::Named(
                        license_matched.clone(),
                        text_with_reference,
                    ));
                    updated.insert(license_matched.clone());
                    replaced_with_blurb = true;

                    // Track rename if the name changed
                    if synopsis != license_matched {
                        renames.insert(synopsis.clone(), license_matched.clone());
                    }
                }
            } else if SPDX_RENAMES.contains_key(synopsis.as_str()) {
                // If no blurb found but it's in SPDX_RENAMES, just record the rename
                let new_name = SPDX_RENAMES[synopsis.as_str()];
                renames.insert(synopsis.clone(), new_name.to_string());
            } else {
                // Found full license text but no matching blurb and not in SPDX_RENAMES
                tracing::debug!(
                    "Found full license text for {}, but unknown synopsis {} ({})",
                    license_matched,
                    synopsis,
                    canonical_id
                );
            }
        } else {
            // No full license text match - check if synopsis looks like a common license
            let common_license_path = Path::new(COMMON_LICENSES_DIR).join(&synopsis);
            if common_license_path.exists() {
                tracing::debug!(
                    "A common license shortname ({}) is used, but license text not recognized.",
                    synopsis
                );
            }
        }

        // Try to add reference to common license (get fresh text after potential replacement)
        // Skip if we just replaced with a blurb - we'll add the reference in the next iteration
        if replaced_with_blurb {
            continue;
        }

        let Some(current_text) = para.text() else {
            continue;
        };

        if let Some(common_license) = find_common_license_from_blurb(&current_text) {
            // Check if already has reference
            if current_text.contains(COMMON_LICENSES_DIR) {
                continue;
            }
            if let Some(comment) = para.comment() {
                if comment.contains(COMMON_LICENSES_DIR) {
                    continue;
                }
            }
            // Check if there's a License-Reference field
            if let Some(license_ref) = para.as_deb822().get("License-Reference") {
                if license_ref.contains(COMMON_LICENSES_DIR) {
                    continue;
                }
            }
            // Check if there's an X-Comment field with reference
            if let Some(x_comment) = para.as_deb822().get("X-Comment") {
                if x_comment.contains(COMMON_LICENSES_DIR) {
                    continue;
                }
            }

            // Check which tags this would fix and if they should be fixed
            let mut should_apply = false;

            // Check common license reference tag
            let common_ref_issue = LintianIssue::source_with_info(
                "copyright-does-not-refer-to-common-license-file",
                vec![common_license.clone()],
            );
            if common_ref_issue.should_fix(base_path) {
                fixed_issues.push(common_ref_issue);
                should_apply = true;
            } else {
                overridden_issues.push(common_ref_issue);
            }

            // Check license-specific tags
            let specific_tag = if common_license.starts_with("Apache-2") {
                Some("copyright-not-using-common-license-for-apache2")
            } else if common_license.starts_with("GPL-") {
                Some("copyright-not-using-common-license-for-gpl")
            } else if common_license.starts_with("LGPL-") {
                Some("copyright-not-using-common-license-for-lgpl")
            } else if common_license.starts_with("GFDL-") {
                Some("copyright-not-using-common-license-for-gfdl")
            } else {
                None
            };

            if let Some(tag) = specific_tag {
                let issue = LintianIssue::source_with_info(tag, vec![common_license.clone()]);
                if issue.should_fix(base_path) {
                    fixed_issues.push(issue);
                    should_apply = true;
                } else {
                    overridden_issues.push(issue);
                }
            }

            // Only apply the change if at least one issue should be fixed
            if should_apply {
                // Add reference
                let license_name = FULL_LICENSE_NAME
                    .get(common_license.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| common_license.clone());
                let reference = debian_file_reference(&license_name, &common_license);
                let new_text = format!("{}\n\n{}", current_text, reference);

                // Apply the change directly
                para.set_license(&License::Named(synopsis.clone(), new_text));

                if synopsis != common_license {
                    renames.insert(synopsis.clone(), common_license.clone());
                }
                updated.insert(common_license);
            }
        }
    }

    // Apply renames in a second pass (both License and Files paragraphs)
    for mut para in copyright.iter_licenses() {
        if let Some(synopsis) = para.name() {
            if let Some(new_synopsis) = renames.get(&synopsis) {
                if let Some(text) = para.text() {
                    para.set_license(&License::Named(new_synopsis.clone(), text));
                }
            }
        }
    }

    // Also update license names in Files paragraphs
    for mut para in copyright.iter_files() {
        if let Some(license) = para.license() {
            let license_name = match &license {
                License::Name(name) => name.as_str(),
                License::Named(name, _) => name.as_str(),
                License::Text(_) => continue,
            };

            if let Some(new_name) = renames.get(license_name) {
                let new_license = match license {
                    License::Name(_) => License::Name(new_name.clone()),
                    License::Named(_, text) => License::Named(new_name.clone(), text),
                    License::Text(text) => License::Text(text),
                };
                para.set_license(&new_license);
            }
        }
    }

    if updated.is_empty() && renames.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Write the modified copyright file
    let new_content = copyright.to_string();
    fs::write(&copyright_path, new_content)?;

    // Build description
    let mut done = Vec::new();
    if !updated.is_empty() {
        let mut sorted: Vec<_> = updated.iter().cloned().collect();
        sorted.sort();
        done.push(format!(
            "refer to common license file for {}",
            sorted.join(", ")
        ));
    }

    let renames_not_updated: HashSet<_> = renames
        .values()
        .filter(|v| !updated.contains(*v))
        .cloned()
        .collect();

    if !renames_not_updated.is_empty() {
        let mut rename_strs: Vec<String> = renames
            .iter()
            .filter(|(_, new)| renames_not_updated.contains(*new))
            .map(|(old, new)| format!("{} (was: {})", new, old))
            .collect();
        rename_strs.sort();
        done.push(format!(
            "use common license names: {}",
            rename_strs.join(", ")
        ));
    }

    let description = if !done.is_empty() {
        let first = done[0].clone();
        let rest = done.join("; ");
        format!(
            "{}{}.",
            first.chars().next().unwrap().to_uppercase(),
            &rest[1..]
        )
    } else {
        "Update copyright file.".to_string()
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "common-license",
    tags: [
        "copyright-does-not-refer-to-common-license-file",
        "copyright-file-contains-full-apache-2-license",
        "copyright-file-contains-full-gfdl-license",
        "copyright-file-contains-full-gpl-license",
        "copyright-not-using-common-license-for-apache2",
        "copyright-not-using-common-license-for-gfdl",
        "copyright-not-using-common-license-for-gpl",
        "copyright-not-using-common-license-for-lgpl"
    ],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_no_copyright() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_not_machine_readable() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "This is not a machine-readable copyright file.\n",
        )
        .unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_blurb_no_leading_spaces() {
        // Verify that blurbs don't have leading spaces
        let apache_blurb = BLURBS.get("Apache-2.0").unwrap();
        let first_line = apache_blurb.lines().next().unwrap();
        assert_eq!(
            first_line.chars().next().unwrap(),
            'L',
            "First line should start with 'L' not a space"
        );
        assert!(
            !first_line.starts_with(' '),
            "Blurb should not have leading spaces"
        );
    }

    #[test]
    fn test_set_license_encoding() {
        // Test that set_license() properly encodes text
        let input = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: Test
 Old text here
"#;
        let (copyright, _) = Copyright::from_str_relaxed(input).unwrap();

        for mut para in copyright.iter_licenses() {
            let new_text = "Line one\nLine two\n\nLine after blank";
            para.set_license(&License::Named("Test".to_string(), new_text.to_string()));
        }

        let output = copyright.to_string();

        let expected = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: Test
 Line one
 Line two
 .
 Line after blank
"#;
        assert_eq!(output, expected);
    }
}
