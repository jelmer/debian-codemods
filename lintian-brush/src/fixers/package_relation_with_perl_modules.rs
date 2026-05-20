use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use std::collections::BTreeSet;
use std::path::PathBuf;

const BINARY_DEP_FIELDS: &[&str] = &[
    "Depends",
    "Pre-Depends",
    "Recommends",
    "Suggests",
    "Enhances",
    "Breaks",
    "Conflicts",
];

const SOURCE_DEP_FIELDS: &[&str] = &[
    "Build-Depends",
    "Build-Depends-Indep",
    "Build-Depends-Arch",
    "Build-Conflicts",
    "Build-Conflicts-Indep",
    "Build-Conflicts-Arch",
];

fn perl_modules_in(value: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    if value.is_empty() {
        return found;
    }
    let (relations, _) = Relations::parse_relaxed(value, true);
    for entry in relations.entries() {
        for relation in entry.relations() {
            if let Some(name) = relation.try_name() {
                if name.starts_with("perl-modules") {
                    found.insert(name);
                }
            }
        }
    }
    found
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

    // Skip the perl source package itself: perl-modules is a real,
    // load-bearing dependency there.
    if let Some(source) = control.source() {
        if source.name().as_deref() == Some("perl") {
            return Ok(Vec::new());
        }
    }

    let mut issues: Vec<LintianIssue> = Vec::new();
    let mut actions: Vec<Action> = Vec::new();

    if let Some(source) = control.source() {
        for field in SOURCE_DEP_FIELDS {
            let value = source.as_deb822().get(field).unwrap_or_default();
            let perl_modules = perl_modules_in(&value);
            if perl_modules.is_empty() {
                continue;
            }
            for name in &perl_modules {
                issues.push(LintianIssue::source_with_info(
                    "package-relation-with-perl-modules",
                    Visibility::Error,
                    vec![format!("{}: {}", field, name)],
                ));
                actions.push(Action::Deb822(Deb822Action::ReplaceRelation {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Source,
                    field: (*field).to_string(),
                    from_package: name.clone(),
                    to_entry: "perl".into(),
                }));
            }
        }
    }

    for binary in control.binaries() {
        let Some(pkg_name) = binary.name() else {
            continue;
        };
        for field in BINARY_DEP_FIELDS {
            let value = binary.as_deb822().get(field).unwrap_or_default();
            let perl_modules = perl_modules_in(&value);
            if perl_modules.is_empty() {
                continue;
            }
            for name in &perl_modules {
                issues.push(LintianIssue::binary_with_info(
                    &pkg_name,
                    "package-relation-with-perl-modules",
                    Visibility::Error,
                    vec![format!("{}: {}", field, name)],
                ));
                actions.push(Action::Deb822(Deb822Action::ReplaceRelation {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: pkg_name.clone(),
                    },
                    field: (*field).to_string(),
                    from_package: name.clone(),
                    to_entry: "perl".into(),
                }));
            }
        }
    }

    if issues.is_empty() {
        return Ok(Vec::new());
    }

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for (i, issue) in issues.into_iter().enumerate() {
        let plan_actions = if i == 0 { actions.clone() } else { Vec::new() };
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "Package relation references perl-modules.",
                "Replace perl-modules dependency with perl.",
                plan_actions,
            )
            .with_certainty(Certainty::Certain),
        );
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "package-relation-with-perl-modules",
    tags: ["package-relation-with-perl-modules"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Source",
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
            paragraph_key: "Source",
            field: "Build-Conflicts-Arch",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package",
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
            field: "Enhances",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Breaks",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Conflicts",
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
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(v.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    fn write_control(content: &str) -> (TempDir, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(&control, content).unwrap();
        (tmp, control)
    }

    #[test]
    fn test_build_depends_fix() {
        let (tmp, control) = write_control(
            "Source: test-pkg\nBuild-Depends: perl-modules, debhelper-compat (= 13)\n\nPackage: test-pkg\nArchitecture: all\nDescription: test\n test\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-pkg\nBuild-Depends: perl, debhelper-compat (= 13)\n\nPackage: test-pkg\nArchitecture: all\nDescription: test\n test\n",
        );
    }

    #[test]
    fn test_build_depends_versioned_perl_modules() {
        let (tmp, control) = write_control(
            "Source: test-pkg\nBuild-Depends: perl-modules-5.28, debhelper-compat (= 13)\n\nPackage: test-pkg\nArchitecture: all\nDescription: test\n test\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-pkg\nBuild-Depends: perl, debhelper-compat (= 13)\n\nPackage: test-pkg\nArchitecture: all\nDescription: test\n test\n",
        );
    }

    #[test]
    fn test_binary_depends_fix() {
        let (tmp, control) = write_control(
            "Source: test-pkg\n\nPackage: test-pkg\nArchitecture: all\nDepends: perl-modules-5.28\nDescription: test\n test\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-pkg\n\nPackage: test-pkg\nArchitecture: all\nDepends: perl\nDescription: test\n test\n",
        );
    }

    #[test]
    fn test_skips_perl_source_package() {
        let (tmp, _) = write_control(
            "Source: perl\nBuild-Depends: perl-modules\n\nPackage: perl-base\nArchitecture: any\nDescription: test\n test\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_no_perl_modules() {
        let (tmp, _) = write_control(
            "Source: test-pkg\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: test-pkg\nArchitecture: all\nDescription: test\n test\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_dedup_perl_after_replacement() {
        let (tmp, control) = write_control(
            "Source: test-pkg\nBuild-Depends: perl, perl-modules\n\nPackage: test-pkg\nArchitecture: all\nDescription: test\n test\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-pkg\nBuild-Depends: perl\n\nPackage: test-pkg\nArchitecture: all\nDescription: test\n test\n",
        );
    }
}
