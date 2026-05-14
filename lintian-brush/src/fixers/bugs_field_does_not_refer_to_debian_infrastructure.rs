use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DEBIAN_BTS: &str = "debbugs://bugs.debian.org";

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
    let Some(bugs) = source.get("Bugs") else {
        return Ok(Vec::new());
    };

    if bugs.contains(".debian.org") {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "bugs-field-does-not-refer-to-debian-infrastructure",
        Visibility::Warning,
        vec![bugs.clone()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Remove Bugs field not pointing to Debian infrastructure.",
        "Remove Bugs field not pointing to Debian infrastructure.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Bugs".into(),
            value: DEBIAN_BTS.into(),
        })],
    )])
}

declare_detector! {
    name: "bugs-field-does-not-refer-to-debian-infrastructure",
    tags: ["bugs-field-does-not-refer-to-debian-infrastructure"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Bugs",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, Some("test".into()), Some("1.0".parse().unwrap()));
        detect(&ws, &FixerPreferences::default())
    }

    fn write_control(base: &Path, content: &str) {
        let debian_dir = base.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        fs::write(debian_dir.join("control"), content).unwrap();
    }

    #[test]
    fn test_non_debian_bugs_field() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBugs: https://github.com/example/foo/issues\n\nPackage: foo\nDescription: Foo\n Foo package\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Remove Bugs field not pointing to Debian infrastructure."
        );

        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nBugs: debbugs://bugs.debian.org\n\nPackage: foo\nDescription: Foo\n Foo package\n",
        );
    }

    #[test]
    fn test_already_debian_bugs_field() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBugs: debbugs://bugs.debian.org\n\nPackage: foo\nDescription: Foo\n Foo package\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_no_bugs_field() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDescription: Foo\n Foo package\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert!(detect_in(temp_dir.path()).unwrap().is_empty());
    }

    #[test]
    fn test_bugs_field_partial_debian_domain() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        // A URL that doesn't contain .debian.org should be flagged
        write_control(
            base,
            "Source: foo\nBugs: https://bugs.example.org/foo\n\nPackage: foo\nDescription: Foo\n Foo package\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Remove Bugs field not pointing to Debian infrastructure."
        );
    }

    #[test]
    fn test_bugs_field_bugs_debian_org_https() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        // A URL with bugs.debian.org should not be flagged
        write_control(
            base,
            "Source: foo\nBugs: https://bugs.debian.org/foo\n\nPackage: foo\nDescription: Foo\n Foo package\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_diagnostic_carries_correct_info() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBugs: https://github.com/example/foo/issues\n\nPackage: foo\nDescription: Foo\n Foo package\n",
        );

        let diagnostics = detect_in(base).unwrap();
        assert_eq!(diagnostics.len(), 1);
        let diag = &diagnostics[0];
        let issue = diag.issue.as_ref().unwrap();
        assert_eq!(
            issue.info.as_deref(),
            Some("https://github.com/example/foo/issues")
        );
        assert_eq!(
            issue.tag.as_deref(),
            Some("bugs-field-does-not-refer-to-debian-infrastructure")
        );
    }

    #[test]
    fn test_overridden_diagnostic() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBugs: https://github.com/example/foo/issues\n\nPackage: foo\nDescription: Foo\n Foo package\n",
        );
        let source_dir = base.join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join("lintian-overrides"),
            "bugs-field-does-not-refer-to-debian-infrastructure\n",
        )
        .unwrap();

        match run_apply(base).unwrap_err() {
            FixerError::NoChangesAfterOverrides(issues) => {
                assert_eq!(issues.len(), 1);
                assert_eq!(
                    issues[0].tag.as_deref(),
                    Some("bugs-field-does-not-refer-to-debian-infrastructure")
                );
            }
            other => panic!("expected NoChangesAfterOverrides, got {:?}", other),
        }
        // Control file must be unchanged.
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nBugs: https://github.com/example/foo/issues\n\nPackage: foo\nDescription: Foo\n Foo package\n",
        );
    }
}
