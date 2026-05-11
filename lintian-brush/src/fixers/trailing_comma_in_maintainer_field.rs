use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
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
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let Some(maintainer) = source.get("Maintainer") else {
        return Ok(Vec::new());
    };
    if !maintainer.trim_end().ends_with(',') {
        return Ok(Vec::new());
    }

    let new_value = maintainer
        .trim_end()
        .trim_end_matches(',')
        .trim_end()
        .to_string();

    let mut issue = LintianIssue::source_with_info(
        "trailing-comma-in-maintainer-field",
        Visibility::Error,
        vec![maintainer.clone()],
    );
    if let Some(package) = ws.package() {
        issue.package = Some(package.to_string());
    }

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Maintainer field has trailing comma.",
        "Remove trailing comma from Maintainer field.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Maintainer".into(),
            value: new_value,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "trailing-comma-in-maintainer-field",
    tags: ["trailing-comma-in-maintainer-field"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Maintainer",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test-package", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_trailing_comma() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: John Doe <john@example.com>,

Package: test-package
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove trailing comma from Maintainer field."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\nMaintainer: John Doe <john@example.com>\n\nPackage: test-package\nDescription: Test package\n Test description\n",
        );
    }

    #[test]
    fn test_no_trailing_comma() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: John Doe <john@example.com>

Package: test-package
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_trailing_comma_with_whitespace() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Jane Smith <jane@example.com> ,

Package: test-package
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\nMaintainer: Jane Smith <jane@example.com>\n\nPackage: test-package\nDescription: Test package\n Test description\n",
        );
        assert_eq!(
            result.description,
            "Remove trailing comma from Maintainer field."
        );
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
