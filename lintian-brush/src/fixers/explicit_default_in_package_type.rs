use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
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

    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        let Some(package_type) = binary.get("Package-Type") else {
            continue;
        };
        if package_type.trim() != "deb" {
            continue;
        }
        let Some(package_name) = binary.name() else {
            continue;
        };

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "explicit-default-in-package-type",
            Visibility::Warning,
            vec![],
        );

        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                format!(
                    "Binary package {} declares the default Package-Type: deb.",
                    package_name
                ),
                format!("Remove redundant Package-Type: deb from {}.", package_name),
                vec![Action::Deb822(Deb822Action::RemoveField {
                    file: PathBuf::from("debian/control"),
                    paragraph: ParagraphSelector::Binary {
                        package: package_name.clone(),
                    },
                    field: "Package-Type".into(),
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut packages: Vec<&str> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Deb822(Deb822Action::RemoveField {
                paragraph: ParagraphSelector::Binary { package },
                field,
                ..
            }) if field == "Package-Type" => Some(package.as_str()),
            _ => None,
        })
        .collect();
    packages.sort();
    packages.dedup();

    if packages.len() == 1 {
        format!("Remove redundant Package-Type: deb from {}.", packages[0])
    } else {
        format!(
            "Remove redundant Package-Type: deb from {}.",
            packages.join(", ")
        )
    }
}

declare_detector! {
    name: "explicit-default-in-package-type",
    tags: ["explicit-default-in-package-type"],
    triggers: [debian_workspace::Trigger::Deb822Field {
        file: "debian/control",
        paragraph_key: "Package",
        field: "Package-Type",
    }],
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
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("example".into()),
            Some(version),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_control(base: &Path, body: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), body).unwrap();
    }

    #[test]
    fn removes_explicit_deb_package_type() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: example\nSection: net\nPriority: optional\n\nPackage: example\nArchitecture: any\nPackage-Type: deb\nDescription: test\n test description\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove redundant Package-Type: deb from example."
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: example\nSection: net\nPriority: optional\n\nPackage: example\nArchitecture: any\nDescription: test\n test description\n",
        );
    }

    #[test]
    fn removes_from_multiple_binaries() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: example\nSection: net\nPriority: optional\n\nPackage: example-one\nArchitecture: any\nPackage-Type: deb\nDescription: one\n .\n\nPackage: example-two\nArchitecture: any\nPackage-Type: deb\nDescription: two\n .\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove redundant Package-Type: deb from example-one, example-two."
        );
    }

    #[test]
    fn leaves_udeb_package_type_alone() {
        let tmp = TempDir::new().unwrap();
        let original = "Source: example\nSection: debian-installer\nPriority: optional\n\nPackage: example-udeb\nArchitecture: any\nPackage-Type: udeb\nDescription: test\n test\n";
        write_control(tmp.path(), original);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            original,
        );
    }

    #[test]
    fn no_change_when_field_absent() {
        let tmp = TempDir::new().unwrap();
        let original = "Source: example\nSection: net\nPriority: optional\n\nPackage: example\nArchitecture: any\nDescription: test\n test\n";
        write_control(tmp.path(), original);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            original,
        );
    }

    #[test]
    fn no_change_when_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn whitespace_around_value_is_tolerated() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: example\nSection: net\nPriority: optional\n\nPackage: example\nArchitecture: any\nPackage-Type:  deb \nDescription: test\n .\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: example\nSection: net\nPriority: optional\n\nPackage: example\nArchitecture: any\nDescription: test\n .\n",
        );
    }
}
