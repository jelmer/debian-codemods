use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        let Some(description) = binary.description() else {
            continue;
        };
        let lines: Vec<&str> = description.split('\n').collect();
        // Empty paragraph at start: short description, then "." continuation,
        // then the rest. The deb822 parser strips the leading space from
        // continuation lines, so a `.` on its own marks the empty paragraph.
        if lines.len() <= 1 || lines[1] != "." {
            continue;
        }
        let Some(package_name) = binary.name() else {
            continue;
        };

        let mut new_lines: Vec<&str> = Vec::with_capacity(lines.len() - 1);
        new_lines.push(lines[0]);
        new_lines.extend(lines.iter().skip(2));
        let new_description = new_lines.join("\n");

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "extended-description-contains-empty-paragraph",
            vec![],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "Remove empty leading paragraph in Description.",
            vec![Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Binary {
                    package: package_name,
                },
                field: "Description".into(),
                value: new_description,
            })],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "extended-description-contains-empty-paragraph",
    tags: ["extended-description-contains-empty-paragraph"],
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
    fn test_empty_paragraph_at_start() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDescription: This is a package\n .\n But it starts with an empty paragraph.\n .\n And then more.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove empty leading paragraph in Description."
        );

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDescription: This is a package\n But it starts with an empty paragraph.\n .\n And then more.\n",
        );
    }

    #[test]
    fn test_no_empty_paragraph() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test\n\nPackage: test\nDescription: This is a package\n With a normal extended description.\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_empty_paragraph_not_at_start() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let original = "Source: test\n\nPackage: test\nDescription: This is a package\n With some text.\n .\n And then more after a separator.\n";
        fs::write(debian.join("control"), original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            original
        );
    }

    #[test]
    fn test_no_description_field() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("control"), "Source: test\n\nPackage: test\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_packages_with_empty_paragraph() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test1\nDescription: First package\n .\n Extended description.\n\nPackage: test2\nDescription: Second package\n .\n Another extended description.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove empty leading paragraph in Description."
        );

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test1\nDescription: First package\n Extended description.\n\nPackage: test2\nDescription: Second package\n Another extended description.\n",
        );
    }
}
