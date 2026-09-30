use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use std::path::PathBuf;

include!(concat!(env!("OUT_DIR"), "/obsolete_packages.rs"));

/// Binary dependency fields lintian's per-installable check treats as
/// dependency fields (matches its `is_dep_field`).
const BINARY_DEP_FIELDS: &[&str] = &["Depends", "Pre-Depends", "Recommends", "Suggests"];

/// Verify that `text` parses as a single simple relation entry naming a
/// valid package. Returns the package name if so. Rejects alternatives,
/// substvars, or free-form prose replacements from the obsolete-packages
/// data file.
fn parse_replacement_package(text: &str) -> Option<String> {
    let (relations, errors) = Relations::parse_relaxed(text, false);
    if !errors.is_empty() {
        return None;
    }
    let entries: Vec<_> = relations.entries().collect();
    if entries.len() != 1 {
        return None;
    }
    let entry = &entries[0];
    let rels: Vec<_> = entry.relations().collect();
    if rels.len() != 1 {
        return None;
    }
    rels[0].try_name()
}

/// A pending edit describing one lintian hint together with the
/// [`Deb822Action`] that would resolve it.
struct PendingEdit {
    /// The obsolete package name lintian sees in this entry (its first
    /// alternative). This is what we emit as the tag's `info` field so
    /// overrides continue to match.
    obsolete_name: String,
    /// Which lintian tag this diagnostic is for. Lintian emits
    /// `ored-depends-on-obsolete-package` when the obsolete package
    /// appears as a non-first alternative and there's a working one
    /// alongside; otherwise `depends-on-obsolete-package`.
    tag: &'static str,
    /// The replacement suggestion from the obsolete-packages data, if
    /// any. Used to render lintian's info text (`Depends: exim => exim4`).
    replacement_info: Option<String>,
    /// The action that resolves this hint. `None` means we recognise the
    /// obsolete package but don't have a confident fix (e.g. the
    /// replacement isn't a package name).
    action: Option<Deb822Action>,
    /// Human-readable label for the fix (for the commit message).
    label: String,
}

/// Return the pending edits for a single relations field.
///
/// Mirrors lintian's per-entry loop in
/// `Lintian/Check/Fields/PackageRelations.pm`: for each comma-separated
/// entry, look at its alternatives. If any alternative names an obsolete
/// package, fire either `depends-on-obsolete-package` (obsolete is the
/// first alternative, or all alternatives are obsolete) or
/// `ored-depends-on-obsolete-package` (obsolete is only in a non-first
/// alternative and at least one working alternative remains).
fn edits_for_field(
    control_rel: &PathBuf,
    paragraph: &ParagraphSelector,
    field: &str,
    value: &str,
) -> Vec<PendingEdit> {
    let mut out = Vec::new();
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    for entry in relations.entries() {
        let alts: Vec<_> = entry.relations().collect();
        if alts.is_empty() {
            continue;
        }
        // Collect obsolete alternatives with their position and metadata.
        let mut obsolete_positions: Vec<usize> = Vec::new();
        let mut alt_names: Vec<Option<String>> = Vec::with_capacity(alts.len());
        for (idx, r) in alts.iter().enumerate() {
            let name = r.try_name();
            if let Some(ref n) = name {
                if obsolete_package_replacement(n).is_some() {
                    obsolete_positions.push(idx);
                }
            }
            alt_names.push(name);
        }
        if obsolete_positions.is_empty() {
            continue;
        }

        // Emit one hint per obsolete alternative, matching lintian's loop
        // over @seen_obsolete_packages.
        for &pos in &obsolete_positions {
            let obsolete_name = alt_names[pos].clone().expect("position implies name");
            let replacement = obsolete_package_replacement(&obsolete_name)
                .expect("position implies obsolete")
                .map(str::to_string);
            let first_is_obsolete = pos == 0;
            let all_obsolete = obsolete_positions.len() == alts.len();
            let tag = if first_is_obsolete || all_obsolete {
                "depends-on-obsolete-package"
            } else {
                "ored-depends-on-obsolete-package"
            };

            let (action, label) = if first_is_obsolete {
                // First-alt obsolete: replace with the suggested package if
                // the suggestion is itself a valid package name.
                if let Some(ref rep) = replacement {
                    if let Some(rep_pkg) = parse_replacement_package(rep) {
                        // Preserve any version constraint from the obsolete relation.
                        let to_entry = match alts[pos].version() {
                            Some((vc, ver)) => format!("{} ({} {})", rep_pkg, vc, ver),
                            None => rep_pkg.clone(),
                        };
                        (
                            Some(Deb822Action::ReplaceRelation {
                                file: control_rel.clone(),
                                paragraph: paragraph.clone(),
                                field: field.to_string(),
                                from_package: obsolete_name.clone(),
                                to_entry,
                            }),
                            format!(
                                "Replace obsolete {} with {} in {}.",
                                obsolete_name, rep_pkg, field
                            ),
                        )
                    } else {
                        (None, String::new())
                    }
                } else {
                    (None, String::new())
                }
            } else {
                // Non-first obsolete alternative: safe to drop it since a
                // working alternative remains. DropAlternative removes
                // just the matching alternative from within its entry;
                // sibling alternatives stay intact.
                (
                    Some(Deb822Action::DropAlternative {
                        file: control_rel.clone(),
                        paragraph: paragraph.clone(),
                        field: field.to_string(),
                        package: obsolete_name.clone(),
                    }),
                    format!(
                        "Drop obsolete alternative {} from {}.",
                        obsolete_name, field
                    ),
                )
            };

            out.push(PendingEdit {
                obsolete_name,
                tag,
                replacement_info: replacement,
                action,
                label,
            });
        }
    }
    out
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let control_rel = PathBuf::from("debian/control");
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for binary in control.binaries() {
        let Some(pkg_name) = binary.name() else {
            continue;
        };
        let paragraph = ParagraphSelector::Binary {
            package: pkg_name.clone(),
        };
        for field in BINARY_DEP_FIELDS {
            let Some(value) = binary.as_deb822().get(field) else {
                continue;
            };
            for edit in edits_for_field(&control_rel, &paragraph, field, &value) {
                // Reconstruct the lintian info string:
                // "<field>: <original entry text>[ => <replacement>]".
                // The lintian info uses the full parsed entry as its
                // "dep" — for a simple relation that's just the package
                // name; we approximate with the obsolete name plus any
                // trailing " => replacement" the data file recorded.
                let mut info = format!("{}: {}", field, edit.obsolete_name);
                if let Some(ref r) = edit.replacement_info {
                    info.push_str(&format!(" => {}", r));
                }
                let issue = LintianIssue {
                    package: Some(pkg_name.clone()),
                    package_type: Some(PackageType::Binary),
                    visibility: Some(Visibility::Warning),
                    tag: Some(edit.tag.to_string()),
                    info: Some(info),
                };
                let summary = format!(
                    "Binary package {} depends on obsolete package {}.",
                    pkg_name, edit.obsolete_name
                );
                let actions = match edit.action {
                    Some(a) => vec![Action::Deb822(a)],
                    None => Vec::new(),
                };
                diagnostics.push(Diagnostic::with_actions(
                    issue, summary, edit.label, actions,
                ));
            }
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut obsolete: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for a in actions {
        match a {
            Action::Deb822(Deb822Action::ReplaceRelation { from_package, .. }) => {
                obsolete.insert(from_package.as_str());
            }
            Action::Deb822(Deb822Action::DropAlternative { package, .. }) => {
                obsolete.insert(package.as_str());
            }
            _ => {}
        }
    }
    if obsolete.len() == 1 {
        let name = obsolete.iter().next().unwrap();
        return format!("Replace or drop obsolete package {}.", name);
    }
    "Replace or drop obsolete package dependencies.".to_string()
}

declare_detector! {
    name: "depends-on-obsolete-package",
    tags: ["depends-on-obsolete-package", "ored-depends-on-obsolete-package"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Pre-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Recommends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Suggests",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = FsWorkspace::new(base, Some("test".into()), Some(v));
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, Some("test".into()), Some("1.0".parse().unwrap()));
        detect(&ws, &FixerPreferences::default())
    }

    fn write_control(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), content).unwrap();
    }

    #[test]
    fn test_parse_replacement_package_simple() {
        assert_eq!(
            parse_replacement_package("exim4"),
            Some("exim4".to_string())
        );
        assert_eq!(
            parse_replacement_package("default-mysql-client"),
            Some("default-mysql-client".to_string()),
        );
    }

    #[test]
    fn test_parse_replacement_package_rejects_prose() {
        assert_eq!(
            parse_replacement_package("use dpkg-buildflags instead"),
            None
        );
        assert_eq!(parse_replacement_package("bsdextrautils and/or ncal"), None,);
        assert_eq!(
            parse_replacement_package("https://wiki.gnome.org/Projects/GnomeCommon/Migration"),
            None,
        );
    }

    #[test]
    fn test_parse_replacement_package_rejects_alternatives() {
        assert_eq!(parse_replacement_package("foo | bar"), None);
    }

    #[test]
    fn test_data_contains_known_entries() {
        // Sanity check that the generated table actually holds entries
        // from data/fields/obsolete-packages.
        assert_eq!(obsolete_package_replacement("exim"), Some(Some("exim4")));
        assert_eq!(
            obsolete_package_replacement("mysql-client"),
            Some(Some("default-mysql-client"))
        );
        // Present but no replacement listed.
        assert!(obsolete_package_replacement("cdrecord").is_some());
        // Not obsolete at all.
        assert_eq!(obsolete_package_replacement("libc6"), None);
    }

    #[test]
    fn test_edits_first_alt_with_replacement() {
        let ctl = PathBuf::from("debian/control");
        let par = ParagraphSelector::Binary {
            package: "foo".into(),
        };
        let edits = edits_for_field(&ctl, &par, "Depends", "exim");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].tag, "depends-on-obsolete-package");
        assert_eq!(edits[0].obsolete_name, "exim");
        assert_eq!(edits[0].replacement_info.as_deref(), Some("exim4"));
        assert!(matches!(
            edits[0].action,
            Some(Deb822Action::ReplaceRelation { ref from_package, ref to_entry, .. })
                if from_package == "exim" && to_entry == "exim4"
        ));
    }

    #[test]
    fn test_edits_first_alt_preserves_version() {
        let ctl = PathBuf::from("debian/control");
        let par = ParagraphSelector::Binary {
            package: "foo".into(),
        };
        let edits = edits_for_field(&ctl, &par, "Depends", "exim (>= 3.0)");
        assert_eq!(edits.len(), 1);
        assert!(matches!(
            edits[0].action,
            Some(Deb822Action::ReplaceRelation { ref to_entry, .. })
                if to_entry == "exim4 (>= 3.0)"
        ));
    }

    #[test]
    fn test_edits_first_alt_prose_replacement_no_action() {
        // hardening-wrapper's replacement is prose, not a package.
        let ctl = PathBuf::from("debian/control");
        let par = ParagraphSelector::Binary {
            package: "foo".into(),
        };
        let edits = edits_for_field(&ctl, &par, "Depends", "hardening-wrapper");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].tag, "depends-on-obsolete-package");
        assert!(edits[0].action.is_none());
    }

    #[test]
    fn test_edits_ored_obsolete_dropped() {
        // Obsolete package as second alternative: safe to drop.
        let ctl = PathBuf::from("debian/control");
        let par = ParagraphSelector::Binary {
            package: "foo".into(),
        };
        let edits = edits_for_field(&ctl, &par, "Depends", "mail-transport-agent | exim");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].tag, "ored-depends-on-obsolete-package");
        assert!(matches!(
            edits[0].action,
            Some(Deb822Action::DropAlternative { ref package, .. }) if package == "exim"
        ));
    }

    #[test]
    fn test_edits_all_alternatives_obsolete_uses_first() {
        // If every alternative is obsolete, lintian tags with the
        // non-ored variant (as in `scalar @seen_obsolete_packages ==
        // scalar @alternatives`).
        let ctl = PathBuf::from("debian/control");
        let par = ParagraphSelector::Binary {
            package: "foo".into(),
        };
        let edits = edits_for_field(&ctl, &par, "Depends", "exim | apache");
        // Two hints, both non-ored per lintian's logic.
        assert_eq!(edits.len(), 2);
        for e in &edits {
            assert_eq!(e.tag, "depends-on-obsolete-package");
        }
    }

    #[test]
    fn test_edits_ignores_modern_packages() {
        let ctl = PathBuf::from("debian/control");
        let par = ParagraphSelector::Binary {
            package: "foo".into(),
        };
        assert!(edits_for_field(&ctl, &par, "Depends", "libc6, python3").is_empty());
        assert!(edits_for_field(&ctl, &par, "Depends", "").is_empty());
    }

    #[test]
    fn test_apply_replaces_in_depends() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: foo\n\nPackage: foo\nArchitecture: any\nDepends: exim, libc6\nDescription: X\n .\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nArchitecture: any\nDepends: exim4, libc6\nDescription: X\n .\n",
        );
    }

    #[test]
    fn test_apply_drops_ored_obsolete() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: foo\n\nPackage: foo\nArchitecture: any\nDepends: mail-transport-agent | exim\nDescription: X\n .\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nArchitecture: any\nDepends: mail-transport-agent\nDescription: X\n .\n",
        );
    }

    #[test]
    fn test_apply_no_change_when_no_obsolete() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: foo\n\nPackage: foo\nArchitecture: any\nDepends: libc6, python3\nDescription: X\n .\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_apply_backs_off_when_replacement_is_prose() {
        // hardening-wrapper's replacement is free-form prose; detector
        // reports the issue but has no action, so the runtime treats it
        // as unfixable.
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: foo\n\nPackage: foo\nArchitecture: any\nDepends: hardening-wrapper, libc6\nDescription: X\n .\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        // Detector still emits the diagnostic (so the tag is
        // acknowledged) even though it can't fix it.
        let diags = detect_in(tmp.path()).unwrap();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
