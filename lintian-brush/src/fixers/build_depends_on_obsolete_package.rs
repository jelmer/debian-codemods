use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::obsolete_packages::obsolete_package_replacement;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_control::lossless::relations::{Relation, Relations};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

const MINIMUM_DEBHELPER_VERSION: &str = "9.20160709";

const FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

/// Render the info lintian reports for an obsolete alternative.
///
/// Lintian reads the field from the .dsc, where dpkg-source has normalised
/// the spacing, so render the relation rather than quoting it verbatim.
fn obsolete_info(field: &str, relation: Relation, replacement: Option<&str>) -> String {
    let relation = debian_control::lossy::Relation::from(relation);
    match replacement {
        Some(replacement) => format!("{field}: {relation} => {replacement}"),
        None => format!("{field}: {relation}"),
    }
}

/// Plan dropping the obsolete alternative `name` from the entry it is in.
fn drop_alternative_plan(control_rel: &Path, field: &str, name: &str) -> ActionPlan {
    ActionPlan {
        label: format!("Drop obsolete alternative {name} from {field}."),
        opinionated: false,
        certainty: None,
        actions: vec![Action::Deb822(Deb822Action::DropAlternative {
            file: control_rel.to_path_buf(),
            paragraph: ParagraphSelector::Source,
            field: field.to_string(),
            package: name.to_string(),
        })],
    }
}

/// Plan replacing dh-systemd with the debhelper version that absorbed it.
fn replace_dh_systemd_plan(control_rel: &Path, field: &str) -> ActionPlan {
    ActionPlan {
        label: format!(
            "Depend on newer debhelper (>= {MINIMUM_DEBHELPER_VERSION}) rather than dh-systemd."
        ),
        opinionated: false,
        certainty: None,
        actions: vec![
            Action::Deb822(Deb822Action::DropRelation {
                file: control_rel.to_path_buf(),
                paragraph: ParagraphSelector::Source,
                field: field.to_string(),
                package: "dh-systemd".into(),
            }),
            Action::Deb822(Deb822Action::EnsureRelation {
                file: control_rel.to_path_buf(),
                paragraph: ParagraphSelector::Source,
                field: "Build-Depends".into(),
                entry: format!("debhelper (>= {MINIMUM_DEBHELPER_VERSION})"),
            }),
        ],
    }
}

/// Return the diagnostics for a single build dependency field.
///
/// Mirrors the per-entry loop in lintian's
/// `Lintian/Check/Fields/PackageRelations.pm`: an obsolete alternative is
/// reported as `ored-build-depends-on-obsolete-package` unless it shares its
/// name with the first alternative or every alternative is obsolete, in
/// which case it is `build-depends-on-obsolete-package`.
fn diagnostics_for_field(control_rel: &Path, field: &str, value: &str) -> Vec<Diagnostic> {
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    let mut diagnostics = Vec::new();
    for entry in relations.entries() {
        let names: Vec<Option<String>> = entry.relations().map(|r| r.try_name()).collect();
        let all_obsolete = names.iter().all(|name| {
            name.as_deref()
                .is_some_and(|n| obsolete_package_replacement(n).is_some())
        });
        let first_name = names.first().cloned().flatten();

        for (relation, name) in entry.relations().zip(&names) {
            let Some(name) = name.as_deref() else {
                continue;
            };
            let Some(replacement) = obsolete_package_replacement(name) else {
                continue;
            };
            let info = obsolete_info(field, relation, replacement);

            let (tag, visibility, description, plan) =
                if first_name.as_deref() != Some(name) && !all_obsolete {
                    // DropAlternative addresses the alternative by package
                    // name and edits the first entry naming it, so back off
                    // when other entries name the package too.
                    let unambiguous = relations.iter_relations_for(name).count() == 1;
                    (
                        "ored-build-depends-on-obsolete-package",
                        Visibility::Info,
                        format!("Build-depends on obsolete {name} package as an alternative."),
                        unambiguous.then(|| drop_alternative_plan(control_rel, field, name)),
                    )
                } else {
                    // TODO: Replace other obsolete packages for which lintian
                    // suggests a replacement package, like the
                    // depends-on-obsolete-package fixer does.
                    let sole_dh_systemd = name == "dh-systemd" && names.len() == 1;
                    (
                        "build-depends-on-obsolete-package",
                        Visibility::Warning,
                        format!("Build-depends on obsolete {name} package."),
                        sole_dh_systemd.then(|| replace_dh_systemd_plan(control_rel, field)),
                    )
                };
            diagnostics.push(Diagnostic::with_plans(
                LintianIssue::source_with_info(tag, visibility, vec![info]),
                description,
                plan.into_iter().collect(),
            ));
        }
    }
    diagnostics
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
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for field in FIELDS {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        diagnostics.extend(diagnostics_for_field(&control_rel, field, &value));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "build-depends-on-obsolete-package",
    tags: [
        "build-depends-on-obsolete-package",
        "ored-build-depends-on-obsolete-package",
    ],
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
    fn test_remove_dh_systemd() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: mypackage\nBuild-Depends: debhelper (>= 9), dh-systemd\n\nPackage: mypackage\nArchitecture: any\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: mypackage\nBuild-Depends: debhelper (>= 9.20160709)\n\nPackage: mypackage\nArchitecture: any\n",
        );
    }

    #[test]
    fn test_no_dh_systemd() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: mypackage\nBuild-Depends: debhelper (>= 9)\n\nPackage: mypackage\nArchitecture: any\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    /// Tag, info and whether a fix is offered for each diagnostic.
    fn summarize(value: &str) -> Vec<(String, String, bool)> {
        diagnostics_for_field(Path::new("debian/control"), "Build-Depends", value)
            .into_iter()
            .map(|d| {
                let issue = d.issue.unwrap();
                (issue.tag.unwrap(), issue.info.unwrap(), !d.plans.is_empty())
            })
            .collect()
    }

    fn apply_build_depends(value: &str) -> Result<String, FixerError> {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            format!("Source: mypackage\nBuild-Depends: {value}\n\nPackage: mypackage\nArchitecture: any\n"),
        )
        .unwrap();
        run_apply(tmp.path())?;
        Ok(fs::read_to_string(&control).unwrap())
    }

    #[test]
    fn test_ored_info_is_normalised() {
        assert_eq!(
            summarize("libfoo-dev |  mkisofs:any  (>=2) [amd64]  <!nocheck>"),
            vec![(
                "ored-build-depends-on-obsolete-package".to_string(),
                "Build-Depends: mkisofs:any (>= 2) [amd64] <!nocheck>".to_string(),
                true
            )],
        );
    }

    #[test]
    fn test_ored_info_has_replacement() {
        assert_eq!(
            summarize("libncurses-dev | libncurses5-dev"),
            vec![(
                "ored-build-depends-on-obsolete-package".to_string(),
                "Build-Depends: libncurses5-dev => libncurses-dev".to_string(),
                true
            )],
        );
    }

    #[test]
    fn test_first_alternative_obsolete_is_not_ored() {
        assert_eq!(
            summarize("libncurses5-dev | libfoo-dev"),
            vec![(
                "build-depends-on-obsolete-package".to_string(),
                "Build-Depends: libncurses5-dev => libncurses-dev".to_string(),
                false
            )],
        );
    }

    #[test]
    fn test_all_alternatives_obsolete_is_not_ored() {
        assert_eq!(
            summarize("xutils | mkisofs"),
            vec![
                (
                    "build-depends-on-obsolete-package".to_string(),
                    "Build-Depends: xutils".to_string(),
                    false
                ),
                (
                    "build-depends-on-obsolete-package".to_string(),
                    "Build-Depends: mkisofs".to_string(),
                    false
                ),
            ],
        );
    }

    #[test]
    fn test_ored_named_by_several_entries_has_no_fix() {
        assert_eq!(
            summarize("mkisofs, genisoimage | mkisofs"),
            vec![
                (
                    "build-depends-on-obsolete-package".to_string(),
                    "Build-Depends: mkisofs".to_string(),
                    false
                ),
                (
                    "ored-build-depends-on-obsolete-package".to_string(),
                    "Build-Depends: mkisofs".to_string(),
                    false
                ),
            ],
        );
    }

    #[test]
    fn test_drop_ored_dh_systemd() {
        assert_eq!(
            apply_build_depends("debhelper (>= 9.20160709) | dh-systemd, pkg-config").unwrap(),
            "Source: mypackage\nBuild-Depends: debhelper (>= 9.20160709), pkg-config\n\nPackage: mypackage\nArchitecture: any\n",
        );
    }

    #[test]
    fn test_drop_ored_listed_twice_in_entry() {
        assert_eq!(
            apply_build_depends("genisoimage | mkisofs [amd64] | mkisofs [i386]").unwrap(),
            "Source: mypackage\nBuild-Depends: genisoimage\n\nPackage: mypackage\nArchitecture: any\n",
        );
    }

    #[test]
    fn test_dh_systemd_with_alternative_is_left_alone() {
        assert!(matches!(
            apply_build_depends("debhelper (>= 9), dh-systemd | systemd"),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_remove_from_build_depends_indep() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: mypackage\nBuild-Depends: debhelper (>= 9)\nBuild-Depends-Indep: dh-systemd\n\nPackage: mypackage\nArchitecture: any\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: mypackage\nBuild-Depends: debhelper (>= 9.20160709)\n\nPackage: mypackage\nArchitecture: any\n",
        );
    }
}
