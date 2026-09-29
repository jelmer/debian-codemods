use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use lazy_regex::regex_is_match;
use std::path::PathBuf;

/// Does this binary package name look like a documentation package?
///
/// Mirrors lintian's `-docs?$` check in
/// `Lintian::Check::Fields::Architecture`.
fn is_documentation_package(package: &str) -> bool {
    regex_is_match!(r"-docs?$", package)
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
    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        let Some(package) = binary.name() else {
            continue;
        };
        if !is_documentation_package(&package) {
            continue;
        }
        // lintian looks at the first architecture entry and reports the
        // tag unless it is exactly "all". A package without an
        // Architecture field is not flagged.
        let arch = binary.as_deb822().get("Architecture");
        match arch.as_deref() {
            None | Some("all") => continue,
            Some(_) => {}
        }

        let issue = LintianIssue::binary_with_info(
            &package,
            "documentation-package-not-architecture-independent",
            Visibility::Warning,
            vec![],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!(
                "documentation package {} is not Architecture: all.",
                package
            ),
            format!("Set Architecture: all on package {}.", package),
            vec![Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Binary {
                    package: package.clone(),
                },
                field: "Architecture".into(),
                value: "all".into(),
            })],
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
                ..
            }) => Some(package.as_str()),
            _ => None,
        })
        .collect();
    packages.sort();
    packages.dedup();

    if packages.len() == 1 {
        format!("Set Architecture: all on package {}.", packages[0])
    } else {
        format!("Set Architecture: all on packages {}.", packages.join(", "))
    }
}

declare_detector! {
    name: "documentation-package-not-architecture-independent",
    tags: ["documentation-package-not-architecture-independent"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Architecture",
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

    #[test]
    fn test_is_documentation_package() {
        assert!(is_documentation_package("foo-doc"));
        assert!(is_documentation_package("foo-docs"));
        assert!(is_documentation_package("libbar-doc"));
        assert!(is_documentation_package("python3-baz-docs"));

        // Not documentation packages.
        assert!(!is_documentation_package("foo"));
        assert!(!is_documentation_package("foo-dev"));
        assert!(!is_documentation_package("doc-foo"));
        assert!(!is_documentation_package("foo-doctor"));
        assert!(!is_documentation_package("foo-documentation"));
    }

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

    fn write_control(contents: &str) -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();
        fs::write(debian_dir.join("control"), contents).unwrap();
        temp_dir
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_set_arch_all_doc() {
        let temp_dir = write_control(
            "Source: foo\n\nPackage: foo-doc\nArchitecture: any\nDescription: Documentation for foo\n Documentation package.\n",
        );
        let control_path = temp_dir.path().join("debian/control");

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Set Architecture: all on package foo-doc."
        );
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: foo\n\nPackage: foo-doc\nArchitecture: all\nDescription: Documentation for foo\n Documentation package.\n",
        );
    }

    #[test]
    fn test_set_arch_all_docs() {
        let temp_dir = write_control(
            "Source: foo\n\nPackage: foo-docs\nArchitecture: amd64\nDescription: Docs for foo\n Documentation package.\n",
        );
        let control_path = temp_dir.path().join("debian/control");

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Set Architecture: all on package foo-docs."
        );
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: foo\n\nPackage: foo-docs\nArchitecture: all\nDescription: Docs for foo\n Documentation package.\n",
        );
    }

    #[test]
    fn test_already_arch_all() {
        let original = "Source: foo\n\nPackage: foo-doc\nArchitecture: all\nDescription: Documentation for foo\n Documentation package.\n";
        let temp_dir = write_control(original);
        let control_path = temp_dir.path().join("debian/control");

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_no_architecture_field() {
        // lintian does not flag a package with no Architecture field.
        let original =
            "Source: foo\n\nPackage: foo-doc\nDescription: Documentation for foo\n Documentation package.\n";
        let temp_dir = write_control(original);

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_non_documentation_package() {
        let original = "Source: foo\n\nPackage: foo-dev\nArchitecture: any\nDescription: Development files for foo\n Dev files.\n";
        let temp_dir = write_control(original);

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_multiple_doc_packages() {
        let temp_dir = write_control(
            "Source: foo\n\nPackage: foo-doc\nArchitecture: any\nDescription: Docs for foo\n Documentation.\n\nPackage: bar-docs\nArchitecture: amd64\nDescription: Docs for bar\n Documentation.\n",
        );
        let control_path = temp_dir.path().join("debian/control");

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Set Architecture: all on packages bar-docs, foo-doc."
        );
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: foo\n\nPackage: foo-doc\nArchitecture: all\nDescription: Docs for foo\n Documentation.\n\nPackage: bar-docs\nArchitecture: all\nDescription: Docs for bar\n Documentation.\n",
        );
    }

    #[test]
    fn test_mixed_packages_only_doc_changed() {
        // A binary that isn't a doc package is left alone; the -doc
        // sibling gets Architecture: all.
        let temp_dir = write_control(
            "Source: foo\n\nPackage: foo\nArchitecture: any\nDescription: The foo program\n Foo.\n\nPackage: foo-doc\nArchitecture: any\nDescription: Docs for foo\n Documentation.\n",
        );
        let control_path = temp_dir.path().join("debian/control");

        run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: foo\n\nPackage: foo\nArchitecture: any\nDescription: The foo program\n Foo.\n\nPackage: foo-doc\nArchitecture: all\nDescription: Docs for foo\n Documentation.\n",
        );
    }
}
