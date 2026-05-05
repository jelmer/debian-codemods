use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;

const MESSAGE: &str = "Change priority extra to priority optional.";

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut diagnostics = Vec::new();

    if let Some(source) = control.source() {
        if source.get("Priority").as_deref() == Some("extra") {
            let issue = LintianIssue::source_with_info(
                "priority-extra-is-replaced-by-priority-optional",
                vec![],
            );
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    MESSAGE,
                    vec![Action::Deb822(Deb822Action::SetField {
                        file: PathBuf::from("debian/control"),
                        paragraph: ParagraphSelector::Source,
                        field: "Priority".into(),
                        value: "optional".into(),
                    })],
                )
                .with_certainty(Certainty::Certain),
            );
        }
    }

    for binary in control.binaries() {
        let Some(package_name) = binary.name() else {
            continue;
        };
        if binary.get("Priority").as_deref() != Some("extra") {
            continue;
        }
        let issue = LintianIssue::binary_with_info(
            &package_name,
            "priority-extra-is-replaced-by-priority-optional",
            vec![],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                MESSAGE,
                vec![Action::Deb822(Deb822Action::SetField {
                    file: PathBuf::from("debian/control"),
                    paragraph: ParagraphSelector::Binary {
                        package: package_name,
                    },
                    field: "Priority".into(),
                    value: "optional".into(),
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "priority-extra-is-replaced-by-priority-optional",
    tags: ["priority-extra-is-replaced-by-priority-optional"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
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
    fn test_change_priority_extra_to_optional() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nPriority: extra\n\nPackage: test-package\nPriority: extra\nDescription: Test package\n This is a test package.\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, MESSAGE);
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\nPriority: optional\n\nPackage: test-package\nPriority: optional\nDescription: Test package\n This is a test package.\n",
        );
    }

    #[test]
    fn test_source_only_priority_extra() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nPriority: extra\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\nPriority: optional\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        );
    }

    #[test]
    fn test_binary_only_priority_extra() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\n\nPackage: test-package\nPriority: extra\nDescription: Test package\n This is a test package.\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\n\nPackage: test-package\nPriority: optional\nDescription: Test package\n This is a test package.\n",
        );
    }

    #[test]
    fn test_no_priority_extra() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: test-package\nPriority: optional\n\nPackage: test-package\nPriority: optional\nDescription: Test package\n This is a test package.\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();
        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
