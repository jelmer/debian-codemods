use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, IndentPattern, ParagraphSelector};
use crate::licenses::{COMMON_LICENSES_DIR, FULL_LICENSE_NAME};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_copyright::lossless::Copyright;
use debian_copyright::License;
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

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

/// Compute the License field's new full value (synopsis + body) for a
/// rewrite. The DEP-5 License field is `<name>\n <body>` with blank
/// lines encoded as `.` (handled by `encode_field_text` upstream).
fn build_license_field(name: &str, body: &str) -> String {
    format!(
        "{}\n{}",
        name,
        debian_copyright::lossless::encode_field_text(body)
    )
}

/// What we're going to do to a single license paragraph.
struct LicensePlan {
    /// Original synopsis (used to address the paragraph).
    original_synopsis: String,
    /// New License field value (`new_synopsis\n<body>`).
    new_license_field: String,
}

/// Compute the rewrite plans for a copyright file along with the lintian
/// issues that motivate them. The detector emits one diagnostic per
/// (issue, paragraph-affected) pair, all carrying the same final SetField
/// for that paragraph — so override filtering is per-issue and the
/// applier deduplicates by value-equality.
pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let bytes = match ws.read_file(&copyright_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };
    let (copyright, _errors) = match Copyright::from_str_relaxed(&content) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("debian/copyright is not machine-readable: {:?}", e);
            return Ok(Vec::new());
        }
    };

    // Per-license-paragraph plans, keyed by original synopsis.
    let mut plans: HashMap<String, LicensePlan> = HashMap::new();
    // Issues attributed to a license-paragraph rewrite (by original synopsis).
    let mut paragraph_issues: HashMap<String, Vec<LintianIssue>> = HashMap::new();
    // Renames recorded purely from SPDX_RENAMES (no body change).
    let mut renames: HashMap<String, String> = HashMap::new();
    // For description formatting.
    let mut updated: HashSet<String> = HashSet::new();

    for para in copyright.iter_licenses() {
        let Some(synopsis) = para.name() else {
            continue;
        };
        let Some(text) = para.text() else {
            continue;
        };
        if text.is_empty() {
            continue;
        }

        // 1. Try to replace full license text with blurb.
        if let Some(license_matched) = find_common_license_from_fulltext(&text) {
            let canonical_id = canonical_license_id(&synopsis);
            let found_blurb = BLURBS
                .iter()
                .find(|(shortname, _)| canonical_id == canonical_license_id(shortname))
                .map(|(_, blurb)| *blurb);

            if let Some(blurb) = found_blurb {
                let license_name = FULL_LICENSE_NAME
                    .get(license_matched.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| license_matched.clone());
                let reference = debian_file_reference(&license_name, &license_matched);
                let text_with_reference = format!("{}\n\n{}", blurb, reference);

                let mut issues: Vec<LintianIssue> = Vec::new();
                if license_matched == "Apache-2.0" {
                    issues.push(LintianIssue::source_with_info(
                        "copyright-file-contains-full-apache-2-license",
                        vec![license_matched.clone()],
                    ));
                }
                if license_matched.starts_with("GFDL-") {
                    issues.push(LintianIssue::source_with_info(
                        "copyright-file-contains-full-gfdl-license",
                        vec![license_matched.clone()],
                    ));
                }
                if license_matched.starts_with("GPL-") {
                    issues.push(LintianIssue::source_with_info(
                        "copyright-file-contains-full-gpl-license",
                        vec![license_matched.clone()],
                    ));
                }
                issues.push(LintianIssue::source_with_info(
                    "copyright-does-not-refer-to-common-license-file",
                    vec![license_matched.clone()],
                ));
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
                    issues.push(LintianIssue::source_with_info(
                        tag,
                        vec![license_matched.clone()],
                    ));
                }

                plans.insert(
                    synopsis.clone(),
                    LicensePlan {
                        original_synopsis: synopsis.clone(),
                        new_license_field: build_license_field(
                            &license_matched,
                            &text_with_reference,
                        ),
                    },
                );
                paragraph_issues
                    .entry(synopsis.clone())
                    .or_default()
                    .extend(issues);
                updated.insert(license_matched.clone());
                if synopsis != license_matched {
                    renames.insert(synopsis.clone(), license_matched.clone());
                }
                continue;
            } else if SPDX_RENAMES.contains_key(synopsis.as_str()) {
                let new_name = SPDX_RENAMES[synopsis.as_str()];
                renames.insert(synopsis.clone(), new_name.to_string());
                continue;
            } else {
                tracing::debug!(
                    "Found full license text for {}, but unknown synopsis {} ({})",
                    license_matched,
                    synopsis,
                    canonical_id
                );
            }
        } else {
            let common_license_path = Path::new(COMMON_LICENSES_DIR).join(&synopsis);
            if common_license_path.exists() {
                tracing::debug!(
                    "A common license shortname ({}) is used, but license text not recognized.",
                    synopsis
                );
            }
        }

        // 2. No fulltext match — try adding a reference to a recognised blurb.
        if let Some(common_license) = find_common_license_from_blurb(&text) {
            if text.contains(COMMON_LICENSES_DIR) {
                continue;
            }
            if let Some(comment) = para.comment() {
                if comment.contains(COMMON_LICENSES_DIR) {
                    continue;
                }
            }
            if let Some(license_ref) = para.as_deb822().get("License-Reference") {
                if license_ref.contains(COMMON_LICENSES_DIR) {
                    continue;
                }
            }
            if let Some(x_comment) = para.as_deb822().get("X-Comment") {
                if x_comment.contains(COMMON_LICENSES_DIR) {
                    continue;
                }
            }

            let license_name = FULL_LICENSE_NAME
                .get(common_license.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| common_license.clone());
            let reference = debian_file_reference(&license_name, &common_license);
            let new_text = format!("{}\n\n{}", text, reference);
            let new_synopsis = if common_license == synopsis {
                synopsis.clone()
            } else {
                common_license.clone()
            };

            let mut issues: Vec<LintianIssue> = Vec::new();
            issues.push(LintianIssue::source_with_info(
                "copyright-does-not-refer-to-common-license-file",
                vec![common_license.clone()],
            ));
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
                issues.push(LintianIssue::source_with_info(
                    tag,
                    vec![common_license.clone()],
                ));
            }

            plans.insert(
                synopsis.clone(),
                LicensePlan {
                    original_synopsis: synopsis.clone(),
                    new_license_field: build_license_field(&new_synopsis, &new_text),
                },
            );
            paragraph_issues
                .entry(synopsis.clone())
                .or_default()
                .extend(issues);
            updated.insert(common_license.clone());
            if synopsis != common_license {
                renames.insert(synopsis.clone(), common_license);
            }
        }
    }

    // For licenses we plan to rename but with no body change yet (those
    // that came from SPDX_RENAMES alone), add a synopsis-only plan so the
    // License paragraph gets renamed.
    for (orig, new_name) in &renames {
        if plans.contains_key(orig) {
            continue;
        }
        // Look up the original License paragraph's body to preserve it.
        let Some(para) = copyright
            .iter_licenses()
            .find(|p| p.name().as_deref() == Some(orig.as_str()))
        else {
            continue;
        };
        let Some(text) = para.text() else { continue };
        plans.insert(
            orig.clone(),
            LicensePlan {
                original_synopsis: orig.clone(),
                new_license_field: build_license_field(new_name, &text),
            },
        );
        // No new lintian issues — this is just a downstream rename.
    }

    if plans.is_empty() {
        return Ok(Vec::new());
    }

    // Build the description that the framework will use.
    let renames_not_updated: HashSet<String> = renames
        .values()
        .filter(|v| !updated.contains(*v))
        .cloned()
        .collect();
    let description = build_description(&updated, &renames, &renames_not_updated);

    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // For each plan, emit one diagnostic per issue (each carrying the
    // SetField for the License paragraph). Plans without lintian issues
    // (downstream renames) get a single untagged diagnostic.
    for plan in plans.values() {
        let action = Action::Deb822(Deb822Action::SetFieldWithIndent {
            file: copyright_rel.clone(),
            paragraph: ParagraphSelector::CopyrightLicense {
                name: plan.original_synopsis.clone(),
            },
            field: "License".to_string(),
            value: plan.new_license_field.clone(),
            indent: IndentPattern::Fixed { spaces: 1 },
        });

        match paragraph_issues.get(&plan.original_synopsis) {
            Some(issues) if !issues.is_empty() => {
                for issue in issues {
                    diagnostics.push(Diagnostic::with_actions(
                        issue.clone(),
                        description.clone(),
                        vec![action.clone()],
                    ));
                }
            }
            _ => {
                diagnostics.push(Diagnostic::untagged(description.clone(), vec![action]));
            }
        }
    }

    // Now update Files paragraphs that reference renamed licenses.
    for para in copyright.iter_files() {
        let Some(license) = para.license() else {
            continue;
        };
        let (license_name, body): (String, Option<String>) = match license {
            License::Name(name) => (name, None),
            License::Named(name, body) => (name, Some(body)),
            License::Text(_) => continue,
        };
        let Some(new_name) = renames.get(&license_name) else {
            continue;
        };
        let Some(glob) = para.as_deb822().get("Files") else {
            continue;
        };
        let new_value = match &body {
            Some(b) if !b.is_empty() => build_license_field(new_name, b),
            _ => new_name.clone(),
        };
        let action = Action::Deb822(Deb822Action::SetFieldWithIndent {
            file: copyright_rel.clone(),
            paragraph: ParagraphSelector::CopyrightFiles { glob },
            field: "License".to_string(),
            value: new_value,
            indent: IndentPattern::Fixed { spaces: 1 },
        });
        diagnostics.push(Diagnostic::untagged(description.clone(), vec![action]));
    }

    Ok(diagnostics)
}

fn build_description(
    updated: &HashSet<String>,
    renames: &HashMap<String, String>,
    renames_not_updated: &HashSet<String>,
) -> String {
    let mut done: Vec<String> = Vec::new();
    if !updated.is_empty() {
        let mut sorted: Vec<_> = updated.iter().cloned().collect();
        sorted.sort();
        done.push(format!(
            "refer to common license file for {}",
            sorted.join(", ")
        ));
    }
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
    if done.is_empty() {
        return "Update copyright file.".to_string();
    }
    let joined = done.join("; ");
    let mut chars = joined.chars();
    match chars.next() {
        Some(c) => format!("{}{}.", c.to_uppercase(), chars.as_str()),
        None => "Update copyright file.".to_string(),
    }
}

declare_detector! {
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
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_no_copyright() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
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

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
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
