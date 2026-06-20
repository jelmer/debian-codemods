use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use std::path::PathBuf;

/// The default-mta virtual package.
const DEFAULT_MTA: &str = "default-mta";

/// The mail-transport-agent virtual package that must accompany it as an
/// alternative.
const MAIL_TRANSPORT_AGENT: &str = "mail-transport-agent";

/// Binary dependency fields lintian checks (its `is_dep_field` set).
const BINARY_DEP_FIELDS: &[&str] = &["Depends", "Pre-Depends", "Recommends", "Suggests"];

/// Source build-dependency fields lintian checks.
const SOURCE_DEP_FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

/// Reproduce lintian's `unfolded_value`: the field value collapsed onto a
/// single logical line.
fn unfolded(value: &str) -> String {
    value.replace('\n', " ").trim().to_string()
}

/// True when `value` names `package` as a predicate anywhere in the
/// field, mirroring lintian's `equals(package, VISIT_PRED_NAME)`.
fn names_package(value: &str, package: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    // Bind the result so the iterator borrowing `relations` is dropped
    // before `relations` itself at the end of the function.
    #[allow(clippy::let_and_return)]
    let found = relations.entries().any(|e| {
        e.relations()
            .filter_map(|r| r.try_name())
            .any(|n| n == package)
    });
    found
}

/// True when `value` depends on default-mta without listing
/// mail-transport-agent — the condition lintian flags.
fn needs_mail_transport_agent(value: &str) -> bool {
    names_package(value, DEFAULT_MTA) && !names_package(value, MAIL_TRANSPORT_AGENT)
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
            if !needs_mail_transport_agent(&value) {
                continue;
            }
            let issue = LintianIssue::source_with_info(
                "default-mta-dependency-does-not-specify-mail-transport-agent",
                Visibility::Warning,
                vec![format!("{}: {}", field, unfolded(&value))],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!(
                    "Add mail-transport-agent alternative to default-mta in {}.",
                    field
                ),
                format!(
                    "Specify mail-transport-agent alternative for default-mta in {}.",
                    field
                ),
                vec![Action::Deb822(Deb822Action::AddAlternative {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Source,
                    field: (*field).to_string(),
                    package: DEFAULT_MTA.to_string(),
                    alternative: MAIL_TRANSPORT_AGENT.to_string(),
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
            if !needs_mail_transport_agent(&value) {
                continue;
            }
            let issue = LintianIssue {
                package: Some(pkg_name.clone()),
                package_type: Some(PackageType::Binary),
                visibility: Some(Visibility::Warning),
                tag: Some(
                    "default-mta-dependency-does-not-specify-mail-transport-agent".to_string(),
                ),
                info: Some(format!("{}: {}", field, unfolded(&value))),
            };
            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!(
                    "Add mail-transport-agent alternative to default-mta in {} of {}.",
                    field, pkg_name
                ),
                format!(
                    "Specify mail-transport-agent alternative for default-mta in {}.",
                    field
                ),
                vec![Action::Deb822(Deb822Action::AddAlternative {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: pkg_name.clone(),
                    },
                    field: (*field).to_string(),
                    package: DEFAULT_MTA.to_string(),
                    alternative: MAIL_TRANSPORT_AGENT.to_string(),
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
            Action::Deb822(Deb822Action::AddAlternative { field, .. }) => Some(field.as_str()),
            _ => None,
        })
        .collect();
    fields.sort();
    fields.dedup();
    match fields.as_slice() {
        [field] => format!(
            "Specify mail-transport-agent alternative for default-mta in {}.",
            field
        ),
        _ => "Specify mail-transport-agent alternative for default-mta.".to_string(),
    }
}

declare_detector! {
    name: "default-mta-dependency-does-not-specify-mail-transport-agent",
    tags: ["default-mta-dependency-does-not-specify-mail-transport-agent"],
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
    fn test_needs_when_default_mta_alone() {
        assert!(needs_mail_transport_agent("default-mta"));
        assert!(needs_mail_transport_agent("libc6, default-mta"));
    }

    #[test]
    fn test_no_need_when_mail_transport_agent_present() {
        assert!(!needs_mail_transport_agent(
            "default-mta | mail-transport-agent"
        ));
        assert!(!needs_mail_transport_agent(
            "default-mta, mail-transport-agent"
        ));
    }

    #[test]
    fn test_no_need_when_no_default_mta() {
        assert!(!needs_mail_transport_agent("foo, bar"));
        assert!(!needs_mail_transport_agent("mail-transport-agent"));
    }

    #[test]
    fn test_adds_alternative_in_depends() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDepends: default-mta\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Specify mail-transport-agent alternative for default-mta in Depends."
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDepends: default-mta | mail-transport-agent\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_adds_alternative_in_build_depends() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: debhelper-compat (= 13), default-mta\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Specify mail-transport-agent alternative for default-mta in Build-Depends."
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nBuild-Depends: debhelper-compat (= 13), default-mta | mail-transport-agent\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_preserves_version_constraint_on_default_mta() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDepends: default-mta (>= 1)\nDescription: Foo\n bar\n",
        );

        run_apply(base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDepends: default-mta (>= 1) | mail-transport-agent\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_detect_reports_field_and_value() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nRecommends: default-mta\nDescription: Foo\n bar\n",
        );

        let diagnostics = detect_in(base).unwrap();
        assert_eq!(diagnostics.len(), 1);
        let issue = diagnostics[0].issue.as_ref().unwrap();
        assert_eq!(issue.package.as_deref(), Some("foo"));
        assert_eq!(issue.package_type, Some(PackageType::Binary));
        assert_eq!(issue.info.as_deref(), Some("Recommends: default-mta"));
    }

    #[test]
    fn test_no_change_when_mail_transport_agent_present() {
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
    fn test_no_change_when_no_default_mta() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDepends: libc6\nDescription: Foo\n bar\n",
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
