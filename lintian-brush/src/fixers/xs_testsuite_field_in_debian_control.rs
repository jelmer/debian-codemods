use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
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
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let paragraph = source.as_deb822();

    let Some(entry) = paragraph.get_entry("XS-Testsuite") else {
        return Ok(Vec::new());
    };
    let line_number = entry.line() + 1;
    let value = paragraph.get("XS-Testsuite").unwrap_or_default();

    let issue = LintianIssue::source_with_info(
        "adopted-extended-field",
        vec![format!(
            "(in section for source) XS-Testsuite [debian/control:{}]",
            line_number
        )],
    );

    let action = if value.trim() == "autopkgtest" {
        // XS-Testsuite: autopkgtest is the default; just drop it.
        Action::Deb822(Deb822Action::RemoveField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "XS-Testsuite".into(),
        })
    } else {
        Action::Deb822(Deb822Action::RenameField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            from: "XS-Testsuite".into(),
            to: "Testsuite".into(),
        })
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        "XS-Testsuite field has been adopted as Testsuite.",
        "Remove unnecessary XS-Testsuite field in debian/control.",
        vec![action],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "xs-testsuite-field-in-debian-control",
    tags: ["adopted-extended-field"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "XS-Testsuite",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
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
    fn test_xs_testsuite_autopkgtest_removed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nXS-Testsuite: autopkgtest\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove unnecessary XS-Testsuite field in debian/control."
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_xs_testsuite_renamed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nXS-Testsuite: custom-test\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nTestsuite: custom-test\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_no_xs_testsuite() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: test\nTestsuite: autopkgtest\n\nPackage: test\nDescription: Test\n Test package\n";
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
}
