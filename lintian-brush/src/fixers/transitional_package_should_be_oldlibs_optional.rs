use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_path = base_path.join(&control_rel);
    if !control_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&control_path)?;
    let control: Control = content.parse().map_err(|_| FixerError::NoChanges)?;

    let source_paragraph = control.source().map(|s| s.as_deb822().clone());
    let default_priority = source_paragraph.as_ref().and_then(|p| p.get("Priority"));
    let default_section = source_paragraph.as_ref().and_then(|p| p.get("Section"));

    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        let paragraph = binary.as_deb822();

        // Skip udebs
        if paragraph.get("Package-Type").as_deref().map(str::trim) == Some("udeb") {
            continue;
        }

        let description = paragraph.get("Description").unwrap_or_default();
        if !description.to_lowercase().contains("transitional package") {
            continue;
        }

        let Some(package_name) = binary.name() else {
            continue;
        };

        let old_section = paragraph.get("Section").or_else(|| default_section.clone());
        let old_priority = paragraph
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
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Binary {
                package: package_name.clone(),
            },
            field: "Section".into(),
            value: new_section,
        })];

        if default_priority.as_deref() == Some("optional") {
            // Source already declares Priority: optional; drop the binary's
            // override so it inherits.
            actions.push(Action::Deb822(Deb822Action::RemoveField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Binary {
                    package: package_name.clone(),
                },
                field: "Priority".into(),
            }));
        } else {
            actions.push(Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
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
                "Move transitional package {} to oldlibs/optional per policy 4.0.1.",
                package_name
            ),
            actions,
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[Diagnostic], actions: &[Action]) -> String {
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

declare_fixer! {
    name: "transitional-package-should-be-oldlibs-optional",
    tags: ["transitional-package-not-oldlibs-optional"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    },
    describe: |fixed, actions| {
        describe_aggregate(fixed, actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
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
