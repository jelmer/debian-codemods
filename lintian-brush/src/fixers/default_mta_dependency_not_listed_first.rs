use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use std::path::PathBuf;

/// The virtual package that must head the set of alternatives.
const DEFAULT_MTA: &str = "default-mta";

/// Binary dependency fields lintian checks (its `is_dep_field` set).
const BINARY_DEP_FIELDS: &[&str] = &["Depends", "Pre-Depends", "Recommends", "Suggests"];

/// Source build-dependency fields lintian checks.
const SOURCE_DEP_FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

/// Reproduce lintian's `unfolded_value`: the field value collapsed onto a
/// single logical line. Continuation-line breaks are dropped; lintian
/// keeps the continuation indentation, and we assume the conventional
/// single-space indent.
fn unfolded(value: &str) -> String {
    value.replace('\n', " ").trim().to_string()
}

/// True when the first relation entry of `value` that names `default-mta`
/// does not list it as the leading alternative — the condition lintian
/// flags.
///
/// Only the first such entry matters: it is the one the
/// [`MakeAlternativePrimary`](Deb822Action::MakeAlternativePrimary)
/// action reorders, and a dependency realistically names `default-mta`
/// in just one alternative group.
fn default_mta_misordered(value: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    for entry in relations.entries() {
        let names: Vec<Option<String>> = entry.relations().map(|r| r.try_name()).collect();
        if names.iter().any(|n| n.as_deref() == Some(DEFAULT_MTA)) {
            return names.first().and_then(|n| n.as_deref()) != Some(DEFAULT_MTA);
        }
    }
    false
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

    if let Some(source) = control.source() {
        let paragraph = source.as_deb822();
        for field in SOURCE_DEP_FIELDS {
            let Some(value) = paragraph.get(field) else {
                continue;
            };
            if !default_mta_misordered(&value) {
                continue;
            }
            let issue = LintianIssue::source_with_info(
                "default-mta-dependency-not-listed-first",
                Visibility::Warning,
                vec![format!("{}: {}", field, unfolded(&value))],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!("default-mta is not listed first in {}.", field),
                format!("Order default-mta first in {}.", field),
                vec![Action::Deb822(Deb822Action::MakeAlternativePrimary {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Source,
                    field: (*field).to_string(),
                    package: DEFAULT_MTA.to_string(),
                })],
            ));
        }
    }

    for binary in control.binaries() {
        let Some(pkg_name) = binary.name() else {
            continue;
        };
        let paragraph = binary.as_deb822();
        for field in BINARY_DEP_FIELDS {
            let Some(value) = paragraph.get(field) else {
                continue;
            };
            if !default_mta_misordered(&value) {
                continue;
            }
            let issue = LintianIssue {
                package: Some(pkg_name.clone()),
                package_type: Some(PackageType::Binary),
                visibility: Some(Visibility::Warning),
                tag: Some("default-mta-dependency-not-listed-first".to_string()),
                info: Some(format!("{}: {}", field, unfolded(&value))),
            };
            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!(
                    "default-mta is not listed first in {} of {}.",
                    field, pkg_name
                ),
                format!("Order default-mta first in {}.", field),
                vec![Action::Deb822(Deb822Action::MakeAlternativePrimary {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: pkg_name.clone(),
                    },
                    field: (*field).to_string(),
                    package: DEFAULT_MTA.to_string(),
                })],
            ));
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut fields: Vec<&str> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Deb822(Deb822Action::MakeAlternativePrimary { field, .. }) => {
                Some(field.as_str())
            }
            _ => None,
        })
        .collect();
    fields.sort();
    fields.dedup();
    match fields.as_slice() {
        [field] => format!("Order default-mta first in {}.", field),
        _ => "Order default-mta first in dependency relations.".to_string(),
    }
}

declare_detector! {
    name: "default-mta-dependency-not-listed-first",
    tags: ["default-mta-dependency-not-listed-first"],
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
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = FsWorkspace::new(base, Some("test".into()), Some(version));
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
    fn test_unfolded_collapses_continuations() {
        assert_eq!(unfolded("foo, bar"), "foo, bar");
        assert_eq!(unfolded("foo,\nbar"), "foo, bar");
        assert_eq!(unfolded("foo, bar  "), "foo, bar");
    }

    #[test]
    fn test_misordered_false_when_default_mta_first() {
        assert!(!default_mta_misordered(
            "default-mta | mail-transport-agent"
        ));
        assert!(!default_mta_misordered("default-mta"));
    }

    #[test]
    fn test_misordered_false_when_no_default_mta() {
        assert!(!default_mta_misordered("foo, bar"));
        assert!(!default_mta_misordered("mail-transport-agent"));
    }

    #[test]
    fn test_misordered_true_when_default_mta_not_first() {
        assert!(default_mta_misordered("mail-transport-agent | default-mta"));
        assert!(default_mta_misordered("foo | bar | default-mta"));
        assert!(default_mta_misordered(
            "libc6, mail-transport-agent | default-mta"
        ));
    }

    #[test]
    fn test_reorders_in_depends() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDepends: mail-transport-agent | default-mta\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(result.description, "Order default-mta first in Depends.");
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDepends: default-mta | mail-transport-agent\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_reorders_in_build_depends() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: debhelper-compat (= 13), mail-transport-agent | default-mta\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Order default-mta first in Build-Depends."
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nBuild-Depends: debhelper-compat (= 13), default-mta | mail-transport-agent\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_detect_reports_field_and_value() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nRecommends: mail-transport-agent | default-mta\nDescription: Foo\n bar\n",
        );

        let diagnostics = detect_in(base).unwrap();
        assert_eq!(diagnostics.len(), 1);
        let issue = diagnostics[0].issue.as_ref().unwrap();
        assert_eq!(issue.package.as_deref(), Some("foo"));
        assert_eq!(issue.package_type, Some(PackageType::Binary));
        assert_eq!(
            issue.info.as_deref(),
            Some("Recommends: mail-transport-agent | default-mta"),
        );
    }

    #[test]
    fn test_no_change_when_default_mta_first() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDepends: default-mta | mail-transport-agent\nDescription: Foo\n bar\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(detect_in(base).unwrap(), vec![]);
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
