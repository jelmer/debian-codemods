use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_control::lossless::relations::{Entry, Relations};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// Relation fields of the source paragraph that are checked for
/// redundancy, paired with their polarity (see [`BINARY_FIELDS`]).
const SOURCE_FIELDS: &[(&str, bool)] = &[
    ("Build-Depends", false),
    ("Build-Depends-Indep", false),
    ("Build-Conflicts", true),
    ("Build-Conflicts-Indep", true),
];

/// Relation fields of a binary paragraph that are checked for redundancy,
/// paired with their polarity.
///
/// `false` marks a *conjunctive* field (Depends-style): every entry must
/// be satisfied, so an entry implied by a stronger sibling is redundant
/// and the weaker one is dropped.
///
/// `true` marks a *disjunctive* field (Conflicts-style): the field fires
/// if any entry matches, so a narrower entry is already covered by a
/// broader sibling and the narrower one is dropped.
///
/// `Provides` and `Enhances` are deliberately excluded: redundancy in
/// those fields has no equivalence-preserving resolution.
const BINARY_FIELDS: &[(&str, bool)] = &[
    ("Depends", false),
    ("Pre-Depends", false),
    ("Recommends", false),
    ("Suggests", false),
    ("Conflicts", true),
    ("Breaks", true),
    ("Replaces", true),
];

/// Whether `x` is made redundant by the presence of `y`.
///
/// Both entries must be single-relation entries: the crate's
/// [`Entry::is_implied_by`] is only sound as an implication test when the
/// implying ("outer") entry carries no alternatives.
fn makes_redundant(x: &Entry, y: &Entry, negative: bool) -> bool {
    if negative {
        // Disjunctive field: `x` is redundant when it is narrower than
        // `y` — i.e. `x` implies `y`, so `y` already covers it.
        y.is_implied_by(x)
    } else {
        // Conjunctive field: `x` is redundant when it is weaker than
        // `y` — i.e. `y` implies `x`, so `y` already guarantees it.
        x.is_implied_by(y)
    }
}

/// Strip redundant entries from a single relation field value.
///
/// Returns the deduplicated field value together with the text of the
/// entries that were dropped, or `None` if the field has no redundancy.
fn dedupe(value: &str, negative: bool) -> Option<(String, Vec<String>)> {
    let (mut relations, _errors) = Relations::parse_relaxed(value, true);
    let entries: Vec<Entry> = relations.entries().collect();

    // Only single-relation entries take part. An alternative group
    // (`a | b`) is treated as opaque and never dropped: implication
    // through alternatives cannot be decided soundly here, and redundant
    // alternative groups are rare in practice.
    let simple = |e: &Entry| e.relations().count() == 1;

    let mut kept: Vec<usize> = Vec::new();
    let mut removed: Vec<usize> = Vec::new();
    for (idx, entry) in entries.iter().enumerate() {
        if !simple(entry) {
            continue;
        }
        if kept
            .iter()
            .any(|&k| makes_redundant(entry, &entries[k], negative))
        {
            removed.push(idx);
            continue;
        }
        // `entry` survives; it may in turn make kept siblings redundant.
        kept.retain(|&k| {
            if makes_redundant(&entries[k], entry, negative) {
                removed.push(k);
                false
            } else {
                true
            }
        });
        kept.push(idx);
    }

    if removed.is_empty() {
        return None;
    }

    removed.sort_unstable();
    let removed_text: Vec<String> = removed
        .iter()
        .map(|&idx| entries[idx].to_string().trim().to_string())
        .collect();
    for &idx in removed.iter().rev() {
        relations.remove_entry(idx);
    }

    Some((relations.to_string(), removed_text))
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut total_removed = 0usize;

    if let Some(source) = control.source() {
        let p = source.as_deb822();
        for &(field, negative) in SOURCE_FIELDS {
            let Some(value) = p.get(field) else {
                continue;
            };
            let Some((new_value, removed)) = dedupe(&value, negative) else {
                continue;
            };
            total_removed += removed.len();
            let issue = LintianIssue::source_with_info(
                "redundant-control-relation",
                Visibility::Pedantic,
                vec![format!(
                    "(in source paragraph) {} {}",
                    field,
                    removed.join(", ")
                )],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                String::new(),
                String::new(),
                vec![Action::Deb822(Deb822Action::SetField {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Source,
                    field: field.to_string(),
                    value: new_value,
                })],
            ));
        }
    }

    for binary in control.binaries() {
        let Some(package) = binary.name() else {
            continue;
        };
        let p = binary.as_deb822();
        for &(field, negative) in BINARY_FIELDS {
            let Some(value) = p.get(field) else {
                continue;
            };
            let Some((new_value, removed)) = dedupe(&value, negative) else {
                continue;
            };
            total_removed += removed.len();
            let issue = LintianIssue::binary_with_info(
                &package,
                "redundant-control-relation",
                Visibility::Pedantic,
                vec![format!(
                    "(in section for {}) {} {}",
                    package,
                    field,
                    removed.join(", ")
                )],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                String::new(),
                String::new(),
                vec![Action::Deb822(Deb822Action::SetField {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: package.clone(),
                    },
                    field: field.to_string(),
                    value: new_value,
                })],
            ));
        }
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    let summary = if total_removed == 1 {
        "Remove redundant relation in debian/control.".to_string()
    } else {
        format!(
            "Remove {} redundant relations in debian/control.",
            total_removed
        )
    };
    for d in &mut diagnostics {
        for plan in &mut d.plans {
            plan.label = summary.clone();
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "redundant-control-relation",
    tags: ["redundant-control-relation"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends-Indep",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Conflicts",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Conflicts-Indep",
        },
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
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Conflicts",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Breaks",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Replaces",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_dedupe_exact_duplicate() {
        let (value, removed) = dedupe("foo, bar, foo", false).unwrap();
        assert_eq!(value, "foo, bar");
        assert_eq!(removed, vec!["foo"]);
    }

    #[test]
    fn test_dedupe_drops_weaker_in_conjunction() {
        // The unversioned relation is implied by the versioned one.
        let (value, removed) = dedupe("foo, foo (>= 1.0)", false).unwrap();
        assert_eq!(value, "foo (>= 1.0)");
        assert_eq!(removed, vec!["foo"]);

        // Order does not matter: the weaker entry is still the one dropped.
        let (value, removed) = dedupe("foo (>= 1.0), foo", false).unwrap();
        assert_eq!(value, "foo (>= 1.0)");
        assert_eq!(removed, vec!["foo"]);
    }

    #[test]
    fn test_dedupe_keeps_strongest_constraint() {
        let (value, removed) = dedupe("foo (>= 1.0), foo (>= 2.0)", false).unwrap();
        assert_eq!(value, "foo (>= 2.0)");
        assert_eq!(removed, vec!["foo (>= 1.0)"]);
    }

    #[test]
    fn test_dedupe_drops_narrower_in_disjunction() {
        // In a Conflicts-style field the broader entry wins.
        let (value, removed) = dedupe("foo (>= 2.0), foo", true).unwrap();
        assert_eq!(value, "foo");
        assert_eq!(removed, vec!["foo (>= 2.0)"]);
    }

    #[test]
    fn test_dedupe_no_redundancy() {
        assert!(dedupe("foo, bar, baz", false).is_none());
        assert!(dedupe("foo (>= 1.0), bar", false).is_none());
    }

    #[test]
    fn test_dedupe_ignores_alternatives() {
        // `foo | bar` is opaque; it is neither dropped nor used to drop.
        assert!(dedupe("foo | bar, foo", false).is_none());
        assert!(dedupe("foo | bar, foo | bar", false).is_none());
    }

    #[test]
    fn test_fix_depends() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDepends: foo, foo (>= 1.0)\nDescription: Test\n Test\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove redundant relation in debian/control."
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDepends: foo (>= 1.0)\nDescription: Test\n Test\n",
        );
    }

    #[test]
    fn test_fix_build_depends() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\nBuild-Depends: debhelper-compat (= 13), debhelper-compat\n\nPackage: test\nDescription: Test\n Test\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: test\nDescription: Test\n Test\n",
        );
    }

    #[test]
    fn test_fix_conflicts_keeps_broader() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nConflicts: foo (>= 2.0), foo\nDescription: Test\n Test\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nConflicts: foo\nDescription: Test\n Test\n",
        );
    }

    #[test]
    fn test_no_redundancy() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test\n\nPackage: test\nDepends: foo, bar\nDescription: Test\n Test\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
