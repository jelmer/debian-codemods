use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let source = control.source();
    let default_priority = source.as_ref().and_then(|s| s.get("Priority"));
    let default_section = source.as_ref().and_then(|s| s.get("Section"));

    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        // Skip udebs
        if binary.get("Package-Type").as_deref().map(str::trim) == Some("udeb") {
            continue;
        }

        let description = binary.get("Description").unwrap_or_default();
        if !description.to_lowercase().contains("transitional package") {
            continue;
        }

        let Some(package_name) = binary.name() else {
            continue;
        };

        let old_section = binary.get("Section").or_else(|| default_section.clone());
        let old_priority = binary
            .get("Priority")
            .or_else(|| default_priority.clone())
            .unwrap_or_else(|| "optional".to_string());

        let info = format!(
            "{}/{}",
            old_section.as_deref().unwrap_or("misc"),
            old_priority
        );

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "transitional-package-not-oldlibs-optional",
            Visibility::Warning,
            vec![info],
        );

        let new_section = match old_section.as_deref() {
            Some(s) => match s.split_once('/') {
                Some((area, _)) => format!("{}/oldlibs", area),
                None => "oldlibs".to_string(),
            },
            None => "oldlibs".to_string(),
        };

        let mut actions = vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Binary {
                package: package_name.clone(),
            },
            field: "Section".into(),
            value: new_section,
        })];

        if default_priority.is_none() || default_priority.as_deref() == Some("optional") {
            // The source default is already optional (whether declared
            // explicitly or left unset); drop the binary's redundant
            // override so it inherits.
            actions.push(Action::Deb822(Deb822Action::RemoveField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Binary {
                    package: package_name.clone(),
                },
                field: "Priority".into(),
            }));
        } else {
            actions.push(Action::Deb822(Deb822Action::SetField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Binary {
                    package: package_name.clone(),
                },
                field: "Priority".into(),
                value: "optional".into(),
            }));
        }

        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!(
                "Transitional package {} is not in oldlibs/optional.",
                package_name
            ),
            format!(
                "Move transitional package {} to oldlibs/optional per policy 4.0.1.",
                package_name
            ),
            actions,
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut packages: Vec<&str> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Deb822(Deb822Action::SetField {
                paragraph: ParagraphSelector::Binary { package },
                field,
                ..
            }) if field == "Section" => Some(package.as_str()),
            _ => None,
        })
        .collect();
    packages.sort();
    packages.dedup();

    if packages.len() == 1 {
        format!(
            "Move transitional package {} to oldlibs/optional per policy 4.0.1.",
            packages[0]
        )
    } else {
        format!(
            "Move transitional packages {} to oldlibs/optional per policy 4.0.1.",
            packages.join(", ")
        )
    }
}

declare_detector! {
    name: "transitional-package-should-be-oldlibs-optional",
    tags: ["transitional-package-not-oldlibs-optional"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Priority",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Section",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package-Type",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Description",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Section",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Priority",
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
    fn test_transitional_package_simple() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: lintian-brush\nPriority: optional\n\nPackage: lintian-brush\nPriority: standard\nSection: libs\nDescription: transitional package for blah\n Test test\n",
        )
        .unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Move transitional package lintian-brush to oldlibs/optional per policy 4.0.1.",
        );

        // Section becomes oldlibs; Priority dropped (source already optional).
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: lintian-brush\nPriority: optional\n\nPackage: lintian-brush\nSection: oldlibs\nDescription: transitional package for blah\n Test test\n",
        );
    }

    #[test]
    fn test_does_not_oscillate_with_redundant_priority_fixer() {
        // Regression test for https://bugs.debian.org/1138774.
        //
        // The aide-dynamic transitional package already sits in oldlibs with a
        // redundant Priority: optional. This fixer must drop that field rather
        // than preserve it, otherwise it fights redundant-priority-optional-field
        // forever (one adds Priority, the other removes it).
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: aide\nSection: admin\nPriority: optional\n\nPackage: aide-dynamic\nPriority: optional\nSection: oldlibs\nDescription: transitional package\n This is a transitional package.\n",
        )
        .unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Move transitional package aide-dynamic to oldlibs/optional per policy 4.0.1.",
        );
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: aide\nSection: admin\nPriority: optional\n\nPackage: aide-dynamic\nSection: oldlibs\nDescription: transitional package\n This is a transitional package.\n",
        );

        // With the binary Priority gone, redundant-priority-optional-field has
        // nothing left to do on the binary, so the two fixers reach a fixed
        // point instead of oscillating.
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            temp_dir.path(),
            Some("aide".into()),
            Some("1.0".parse().unwrap()),
        );
        let redundant = crate::fixers::redundant_priority_optional_field::detect(
            &ws,
            &FixerPreferences::default(),
        )
        .unwrap();
        let touches_binary = redundant.iter().any(|d| {
            d.plans.iter().flat_map(|p| &p.actions).any(|a| {
                matches!(
                    a,
                    Action::Deb822(Deb822Action::RemoveField {
                        paragraph: ParagraphSelector::Binary { .. },
                        ..
                    })
                )
            })
        });
        assert!(!touches_binary);

        // And this fixer makes no further change on its own output.
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_transitional_package_drops_priority_when_source_priority_unset() {
        // The source stanza declares no Priority, so the default is
        // optional. A binary's explicit Priority: optional is therefore
        // redundant and must be dropped, not preserved -- otherwise this
        // fixer and redundant-priority-optional-field oscillate forever.
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: lintian-brush\n\nPackage: lintian-brush\nPriority: optional\nSection: libs\nDescription: transitional package for blah\n Test test\n",
        )
        .unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Move transitional package lintian-brush to oldlibs/optional per policy 4.0.1.",
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: lintian-brush\n\nPackage: lintian-brush\nSection: oldlibs\nDescription: transitional package for blah\n Test test\n",
        );
    }

    #[test]
    fn test_transitional_package_with_area() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: lintian-brush\nPriority: optional\n\nPackage: lintian-brush\nPriority: standard\nSection: contrib/libs\nDescription: transitional package for blah\n Test test\n",
        )
        .unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Move transitional package lintian-brush to oldlibs/optional per policy 4.0.1.",
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: lintian-brush\nPriority: optional\n\nPackage: lintian-brush\nSection: contrib/oldlibs\nDescription: transitional package for blah\n Test test\n",
        );
    }

    #[test]
    fn test_skip_udeb() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: gdk-pixbuf\nSection: libs\nPriority: optional\n\nPackage: libgdk-pixbuf2.0-0-udeb\nPackage-Type: udeb\nSection: debian-installer\nDescription: GDK Pixbuf library - minimal runtime\n This transitional package depends on libgdk-pixbuf-2.0-0-udeb.\n",
        )
        .unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_change_when_not_transitional() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: lintian-brush\nPriority: optional\n\nPackage: lintian-brush\nSection: libs\nDescription: A real package\n Test test\n",
        )
        .unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_change_when_no_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
