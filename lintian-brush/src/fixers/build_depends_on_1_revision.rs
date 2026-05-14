use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_control::lossless::relations::Relations;
use debian_control::relations::VersionConstraint;
use debian_workspace::Workspace;
use std::path::PathBuf;
use std::str::FromStr;

const SOURCE_DEP_FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

/// If `version` is a `-1`-suffixed Debian revision, return its upstream-only
/// form (everything before the trailing `-1`). Returns `None` otherwise.
fn strip_dash_one(version: &str) -> Option<&str> {
    version.strip_suffix("-1")
}

/// For the relations text `value`, return a list of `(package_name, new_entry_text)`
/// pairs describing how each entry that contains a `>= X-1` relation should
/// be rewritten. The new entry text preserves any alternatives, but the `-1`
/// suffix is dropped from the offending relation.
fn rewrites(value: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    for entry in relations.entries() {
        // Find the first offending relation in this entry. We rewrite a
        // single entry at a time; multiple offending relations within one
        // entry are unusual enough that handling them in a single pass would
        // complicate the action model without a real-world payoff.
        let Some((rel_idx, offending_relation)) =
            entry
                .relations()
                .enumerate()
                .find(|(_, r)| match r.version() {
                    Some((vc, ver)) => {
                        matches!(vc, VersionConstraint::GreaterThanEqual)
                            && ver.to_string().ends_with("-1")
                    }
                    None => false,
                })
        else {
            continue;
        };

        let Some(offending_name) = offending_relation.try_name() else {
            continue;
        };
        let Some((_, ver)) = offending_relation.version() else {
            continue;
        };
        let ver_str = ver.to_string();
        let Some(new_ver_str) = strip_dash_one(&ver_str) else {
            continue;
        };

        // Build the rewritten entry by mutating a fresh parse of just this
        // entry's text — we can't mutate the iterator's `Relation` in place.
        let entry_text = entry.to_string();
        let Ok(entry_mut) = debian_control::lossless::relations::Entry::from_str(&entry_text)
        else {
            continue;
        };
        let Some(mut rel_mut) = entry_mut.get_relation(rel_idx) else {
            continue;
        };
        let Ok(new_version) = debversion::Version::from_str(new_ver_str) else {
            continue;
        };
        rel_mut.set_version(Some((VersionConstraint::GreaterThanEqual, new_version)));

        out.push((offending_name, entry_mut.to_string()));
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
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    let control_rel = PathBuf::from("debian/control");
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for field in SOURCE_DEP_FIELDS {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        for (package, new_entry) in rewrites(&value) {
            let issue = LintianIssue::source_with_info(
                "build-depends-on-1-revision",
                Visibility::Warning,
                vec![format!("{}: {}", field, package)],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!(
                    "Build dependency on {} pins -1 revision unnecessarily.",
                    package
                ),
                format!("Drop -1 revision from {} build dependency.", package),
                vec![Action::Deb822(Deb822Action::ReplaceRelation {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Source,
                    field: (*field).to_string(),
                    from_package: package,
                    to_entry: new_entry,
                })],
            ));
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut packages: Vec<&str> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Deb822(Deb822Action::ReplaceRelation { from_package, .. }) => {
                Some(from_package.as_str())
            }
            _ => None,
        })
        .collect();
    packages.sort();
    packages.dedup();

    match packages.as_slice() {
        [pkg] => format!("Drop -1 revision from {} build dependency.", pkg),
        pkgs => format!(
            "Drop -1 revision from build dependencies on {}.",
            pkgs.join(", ")
        ),
    }
}

declare_detector! {
    name: "build-depends-on-1-revision",
    tags: ["build-depends-on-1-revision"],
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
            field: "Build-Depends-Arch",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, "test", "1.0".parse().unwrap());
        detect(&ws, &FixerPreferences::default())
    }

    fn write_control(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), content).unwrap();
    }

    #[test]
    fn test_rewrites_simple_dash_one() {
        let r = rewrites("libfoo (>= 1.2-1)");
        assert_eq!(
            r,
            vec![("libfoo".to_string(), "libfoo (>= 1.2)".to_string())]
        );
    }

    #[test]
    fn test_rewrites_preserves_alternatives() {
        let r = rewrites("libfoo (>= 1.2-1) | bar");
        assert_eq!(
            r,
            vec![("libfoo".to_string(), "libfoo (>= 1.2) | bar".to_string())]
        );
    }

    #[test]
    fn test_rewrites_ignores_other_constraints() {
        // Only `>=` triggers the tag.
        assert!(rewrites("libfoo (= 1.2-1)").is_empty());
        assert!(rewrites("libfoo (<< 1.2-1)").is_empty());
        assert!(rewrites("libfoo (>> 1.2-1)").is_empty());
    }

    #[test]
    fn test_rewrites_ignores_non_dash_one_versions() {
        assert!(rewrites("libfoo (>= 1.2)").is_empty());
        assert!(rewrites("libfoo (>= 1.2-2)").is_empty());
        assert!(rewrites("libfoo (>= 1.2-1~)").is_empty());
    }

    #[test]
    fn test_rewrites_handles_multiple_entries() {
        let r = rewrites("libfoo (>= 1.2-1), libbar (>= 2.0-1)");
        assert_eq!(
            r,
            vec![
                ("libfoo".to_string(), "libfoo (>= 1.2)".to_string()),
                ("libbar".to_string(), "libbar (>= 2.0)".to_string()),
            ]
        );
    }

    #[test]
    fn test_strips_dash_one_in_build_depends() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: libfoo (>= 1.2-1)\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Drop -1 revision from libfoo build dependency."
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nBuild-Depends: libfoo (>= 1.2)\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_aggregates_multiple_packages() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: libfoo (>= 1.2-1), libbar (>= 2.0-1)\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Drop -1 revision from build dependencies on libbar, libfoo."
        );
        assert_eq!(result.fixed_lintian_issues.len(), 2);
    }

    #[test]
    fn test_also_handles_build_depends_indep_and_arch() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends-Indep: libindep (>= 1.0-1)\nBuild-Depends-Arch: libarch (>= 2.0-1)\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 2);
        let content = fs::read_to_string(base.join("debian/control")).unwrap();
        assert!(content.contains("Build-Depends-Indep: libindep (>= 1.0)"));
        assert!(content.contains("Build-Depends-Arch: libarch (>= 2.0)"));
    }

    #[test]
    fn test_no_change_when_no_build_depends() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let content = "Source: foo\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_no_change_when_no_dash_one() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let content =
            "Source: foo\nBuild-Depends: libfoo (>= 1.2), libbar (>= 2.0-2)\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_diagnostic_carries_correct_info() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: libfoo (>= 1.2-1)\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let diags = detect_in(base).unwrap();
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert_eq!(issue.tag.as_deref(), Some("build-depends-on-1-revision"));
        assert_eq!(issue.info.as_deref(), Some("Build-Depends: libfoo"));
    }
}
