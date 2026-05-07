use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_copyright::lossless::Copyright;
use std::collections::HashSet;
use std::path::PathBuf;
use tracing::warn;

const VALID_FIELD_NAMES: &[&str] = &[
    "Files",
    "License",
    "Copyright",
    "Comment",
    "Upstream-Name",
    "Format",
    "Upstream-Contact",
    "Source",
    "Upstream",
    "Contact",
    "Name",
];

/// One renamed field, tagged with how the rename was inferred.
#[derive(Clone, Debug)]
struct Rename {
    old: String,
    new: String,
    /// True if the rename only differs in case.
    is_case: bool,
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
    let (copyright, _errors) = match Copyright::from_str_relaxed(&content) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("debian/copyright is not machine-readable: {:?}", e);
            return Ok(Vec::new());
        }
    };
    let deb822 = copyright.as_deb822();

    let valid_fields: HashSet<&str> = VALID_FIELD_NAMES.iter().copied().collect();
    let mut diagnostics = Vec::new();

    for (index, paragraph) in deb822.paragraphs().enumerate() {
        let field_names: Vec<String> = paragraph.keys().collect();
        for field_name in field_names {
            if valid_fields.contains(field_name.as_str()) {
                continue;
            }

            let Some(rename) = infer_rename(&paragraph, &field_name, &valid_fields) else {
                continue;
            };

            let mut actions = vec![Action::Deb822(Deb822Action::RenameField {
                file: copyright_rel.clone(),
                paragraph: ParagraphSelector::Index { index },
                from: rename.old.clone(),
                to: rename.new.clone(),
            })];

            // Case-only renames don't carry a lintian tag — they're
            // cosmetic. Typos do.
            let issue = if rename.is_case {
                None
            } else {
                Some(LintianIssue::source_with_info(
                    "field-name-typo-in-dep5-copyright",
                    vec![rename.old.clone()],
                ))
            };

            // Stash the kind in the plan label so the describer can split
            // case from typo without re-deriving it.
            let label = format!(
                "{}\t{} ⇒ {}",
                if rename.is_case { "case" } else { "typo" },
                rename.old,
                rename.new,
            );
            let description = if rename.is_case {
                format!(
                    "Field name {} has wrong case (should be {}).",
                    rename.old, rename.new
                )
            } else {
                format!(
                    "Field name {} appears to be a typo for {}.",
                    rename.old, rename.new
                )
            };

            let diag = match issue {
                Some(i) => Diagnostic::with_actions(i, description, label, actions.clone()),
                None => {
                    crate::diagnostic::Diagnostic::untagged(description, label, actions.clone())
                }
            };
            diagnostics.push(diag);
            // Mute unused-let warning; `actions` was cloned just so it
            // outlives the conditional above.
            let _ = &mut actions;
        }
    }

    Ok(diagnostics)
}

fn infer_rename(
    paragraph: &deb822_lossless::Paragraph,
    field_name: &str,
    valid_fields: &HashSet<&str>,
) -> Option<Rename> {
    // X- prefix: drop the prefix if the unprefixed name is valid.
    if let Some(without_prefix) = field_name.strip_prefix("X-") {
        if valid_fields.contains(without_prefix) {
            if paragraph.get(without_prefix).is_some() {
                warn!("Both {} and {} exist.", field_name, without_prefix);
                return None;
            }
            return Some(Rename {
                old: field_name.to_string(),
                new: without_prefix.to_string(),
                is_case: false,
            });
        }
    }

    // Levenshtein distance == 1 from a valid field name.
    for &valid_field in VALID_FIELD_NAMES {
        if strsim::levenshtein(field_name, valid_field) != 1 {
            continue;
        }

        let is_case = valid_field.eq_ignore_ascii_case(field_name);
        if let Some(existing) = paragraph.get(valid_field) {
            // Target field already present: only safe to rename if it's a
            // pure case change *and* the values are equal (the rename is a
            // no-op then anyway). Otherwise bail.
            if !is_case {
                warn!(
                    "Found typo ({} ⇒ {}), but {} already exists",
                    field_name, valid_field, valid_field
                );
                return None;
            }
            let value = paragraph.get(field_name)?;
            if value != existing {
                warn!(
                    "Found typo ({} ⇒ {}), but {} already exists",
                    field_name, valid_field, valid_field
                );
                return None;
            }
        }

        return Some(Rename {
            old: field_name.to_string(),
            new: valid_field.to_string(),
            is_case,
        });
    }

    None
}

/// Build the aggregate description from the diagnostics that fired.
///
/// The original wording: `Fix field name {kind} in debian/copyright (X ⇒ Y, ...).`
/// where `{kind}` is "case", "cases", "typo", "typos", or "case and typo"
/// (with appropriate plurals) depending on how many of each fired.
fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let mut case_pairs: Vec<(String, String)> = Vec::new();
    let mut typo_pairs: Vec<(String, String)> = Vec::new();
    for (diag, _) in fixed {
        let Some(plan) = diag.plans.first() else {
            continue;
        };
        let Some((kind, rest)) = plan.label.split_once('\t') else {
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

    let kind_str = match (!case_pairs.is_empty(), !typo_pairs.is_empty()) {
        (true, true) => format!(
            "{} and {}",
            if case_pairs.len() > 1 {
                "cases"
            } else {
                "case"
            },
            if typo_pairs.len() > 1 {
                "typos"
            } else {
                "typo"
            },
        ),
        (true, false) => {
            if case_pairs.len() > 1 {
                "cases".to_string()
            } else {
                "case".to_string()
            }
        }
        (false, true) => {
            if typo_pairs.len() > 1 {
                "typos".to_string()
            } else {
                "typo".to_string()
            }
        }
        (false, false) => String::new(),
    };

    let mut all = case_pairs;
    all.extend(typo_pairs);
    all.sort();
    let fixed_str = all
        .iter()
        .map(|(old, new)| format!("{} ⇒ {}", old, new))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "Fix field name {} in debian/copyright ({}).",
        kind_str, fixed_str
    )
}

declare_detector! {
    name: "field-name-typo-in-dep5-copyright",
    tags: ["field-name-typo-in-dep5-copyright"],
    // Must fix field name typos before copyright format updates
    before: ["out-of-date-copyright-format-uri"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Format",
            field: "*",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Files",
            field: "*",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "License",
            field: "*",
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
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_simple_typo() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\n\nFile: *\nCopyright:\n 2008-2017 Somebody\nLicense: GPL-2+\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Fix field name typo in debian/copyright (File ⇒ Files)."
        );
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("field-name-typo-in-dep5-copyright")
        );

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: GPL-2+\n",
        );
    }

    #[test]
    fn test_case_fix() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-name: lintrian\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: GPL-2+\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Fix field name case in debian/copyright (Upstream-name ⇒ Upstream-Name)."
        );
        // Case-only renames don't get a lintian tag.
        assert_eq!(result.fixed_lintian_issues.len(), 0);

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: GPL-2+\n",
        );
    }

    #[test]
    fn test_x_field() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: GPL-2+\nX-Comment: blah\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Fix field name typo in debian/copyright (X-Comment ⇒ Comment)."
        );
        assert_eq!(result.fixed_lintian_issues.len(), 1);

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: GPL-2+\nComment: blah\n",
        );
    }

    #[test]
    fn test_not_machine_readable() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "This is not a machine-readable copyright file.\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_needed() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: GPL-2+\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
}
