use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
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
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    if source.get("Testsuite").as_deref() != Some("autopkgtest") {
        return Ok(Vec::new());
    }

    let line_number = source
        .as_deb822()
        .get_entry("Testsuite")
        .map(|e| e.line() + 1)
        .unwrap_or(1);

    let issue = LintianIssue::source_with_info(
        "unnecessary-testsuite-autopkgtest-field",
        vec![format!("[debian/control:{}]", line_number)],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Remove unnecessary 'Testsuite: autopkgtest' header.",
        vec![Action::Deb822(Deb822Action::RemoveField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Testsuite".into(),
        })],
    )])
}

declare_detector! {
    name: "unnecessary-testsuite-autopkgtest-field",
    tags: ["unnecessary-testsuite-autopkgtest-field"],
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
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_remove_autopkgtest_testsuite() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: lintian-brush\nTestsuite: autopkgtest\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove unnecessary 'Testsuite: autopkgtest' header."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        );
    }

    #[test]
    fn test_no_testsuite_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: lintian-brush\n\nPackage: lintian-brush\n",
        )
        .unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_different_testsuite_value() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: lintian-brush\nTestsuite: other-value\n\nPackage: lintian-brush\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }
}
