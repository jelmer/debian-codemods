use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let default_priority = control.source().as_ref().and_then(|s| s.get("Priority"));

    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        if binary.get("Section").as_deref() != Some("libs") {
            continue;
        }

        let binary_priority = binary.get("Priority");
        let effective_priority = binary_priority
            .clone()
            .or_else(|| default_priority.clone())
            .unwrap_or_default();
        if !matches!(
            effective_priority.as_str(),
            "required" | "important" | "standard"
        ) {
            continue;
        }
        let Some(package_name) = binary.name() else {
            continue;
        };

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "excessive-priority-for-library-package",
            vec![effective_priority.clone()],
        );

        // If the binary has its own Priority and the source's effective
        // priority would already be `optional` once the binary's override
        // is gone, we can just drop the field instead of explicitly
        // setting it. Otherwise we need an explicit `Priority: optional`
        // to override whatever the source declares.
        let action = if binary_priority.is_some() && default_priority.as_deref() == Some("optional")
        {
            Action::Deb822(Deb822Action::RemoveField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Binary {
                    package: package_name.clone(),
                },
                field: "Priority".into(),
            })
        } else {
            Action::Deb822(Deb822Action::SetField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Binary {
                    package: package_name.clone(),
                },
                field: "Priority".into(),
                value: "optional".into(),
            })
        };

        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("Library package {} has excessive priority.", package_name),
            format!(
                "Set priority for library package {} to optional.",
                package_name
            ),
            vec![action],
        ));
    }

    Ok(diagnostics)
}

/// Custom describer: aggregate all affected library package names into a
/// single line so multi-package fixes get
/// "Set priority for library packages X, Y to optional." instead of one
/// line per package.
fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut packages: Vec<&str> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Deb822(
                Deb822Action::SetField {
                    paragraph: ParagraphSelector::Binary { package },
                    ..
                }
                | Deb822Action::RemoveField {
                    paragraph: ParagraphSelector::Binary { package },
                    ..
                },
            ) => Some(package.as_str()),
            _ => None,
        })
        .collect();
    packages.sort();
    packages.dedup();

    if packages.len() == 1 {
        format!(
            "Set priority for library package {} to optional.",
            packages[0]
        )
    } else {
        format!(
            "Set priority for library packages {} to optional.",
            packages.join(", ")
        )
    }
}

declare_detector! {
    name: "excessive-priority-for-library-package",
    tags: ["excessive-priority-for-library-package"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Priority",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Section",
        },
        crate::workspace::Trigger::Deb822Field {
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
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_simple_library_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: bzip2\nPriority: required\n\nPackage: libbzip2\nSection: libs\nPriority: required\nDescription: blah blah\n blah\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Set priority for library package libbzip2 to optional."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: bzip2\nPriority: required\n\nPackage: libbzip2\nSection: libs\nPriority: optional\nDescription: blah blah\n blah\n",
        );
    }

    #[test]
    fn test_implied_priority_from_source() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: bzip2\nPriority: required\n\nPackage: libbzip2\nSection: libs\nDescription: blah blah\n blah\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Set priority for library package libbzip2 to optional."
        );

        // Source priority is preserved; the binary gets Priority: optional
        // inserted at the canonical position (after Section, before Description).
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: bzip2\nPriority: required\n\nPackage: libbzip2\nSection: libs\nPriority: optional\nDescription: blah blah\n blah\n",
        );
    }

    #[test]
    fn test_multiple_library_packages() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nPriority: standard\n\nPackage: libtest1\nSection: libs\nPriority: important\nDescription: Test 1\n Test\n\nPackage: libtest2\nSection: libs\nDescription: Test 2\n Test\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Set priority for library packages libtest1, libtest2 to optional."
        );
        assert_eq!(result.fixed_lintian_issues.len(), 2);

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nPriority: standard\n\nPackage: libtest1\nSection: libs\nPriority: optional\nDescription: Test 1\n Test\n\nPackage: libtest2\nSection: libs\nPriority: optional\nDescription: Test 2\n Test\n",
        );
    }

    #[test]
    fn test_non_library_package_unchanged() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: test\n\nPackage: test-app\nSection: utils\nPriority: required\nDescription: Test app\n Test\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_library_package_already_optional() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: test\n\nPackage: libtest\nSection: libs\nPriority: optional\nDescription: Test\n Test\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
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
    fn test_remove_when_source_default_is_optional() {
        // Source declares Priority: optional. A binary explicitly setting
        // Priority: required is excessive — and the right fix is to
        // remove the binary's override, not to also write Priority: optional.
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nPriority: optional\n\nPackage: libtest\nSection: libs\nPriority: required\nDescription: Test\n Test\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Set priority for library package libtest to optional."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nPriority: optional\n\nPackage: libtest\nSection: libs\nDescription: Test\n Test\n",
        );
    }
}
