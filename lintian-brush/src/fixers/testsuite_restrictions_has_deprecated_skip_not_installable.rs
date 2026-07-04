use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use deb822_lossless::Deb822;
use debian_workspace::Workspace;
use std::path::PathBuf;

const DEPRECATED: &str = "skip-not-installable";

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/tests/control");
    let bytes = match ws.read_file(&control_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };

    let content = std::str::from_utf8(&bytes)
        .map_err(|e| FixerError::Other(format!("debian/tests/control is not UTF-8: {}", e)))?;
    let parsed = Deb822::parse(content);
    let deb822 = parsed.tree();

    let mut actions = Vec::new();

    for (index, paragraph) in deb822.paragraphs().enumerate() {
        let Some(restrictions_entry) = paragraph
            .entries()
            .find(|e| e.key().as_deref() == Some("Restrictions"))
        else {
            continue;
        };

        let restrictions: Vec<String> = restrictions_entry
            .value()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if !restrictions.iter().any(|r| r == DEPRECATED) {
            continue;
        }

        let kept: Vec<String> = restrictions
            .into_iter()
            .filter(|r| r != DEPRECATED)
            .collect();

        let selector = ParagraphSelector::Index { index };
        let action = if kept.is_empty() {
            Action::Deb822(Deb822Action::RemoveField {
                file: control_rel.clone(),
                paragraph: selector,
                field: "Restrictions".into(),
            })
        } else {
            Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: selector,
                field: "Restrictions".into(),
                value: kept.join(", "),
            })
        };
        actions.push(action);
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    // lintian emits a single hint per debian/tests/control, pointing at the
    // file itself, regardless of how many paragraphs carry the restriction.
    let issue = LintianIssue::source_with_info(
        "testsuite-restrictions-has-deprecated-skip-not-installable",
        Visibility::Warning,
        vec!["[debian/tests/control]".to_string()],
    );

    // Only "possible": skip-not-installable skips a test at runtime when its
    // dependencies aren't installable (e.g. on some architectures). Lintian
    // recommends expressing that intent through the Architecture field
    // instead, but we can't infer which architectures were meant. Merely
    // dropping the restriction makes a previously-skipped test run and
    // potentially fail, so we can't claim this is a safe mechanical change.
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Drop deprecated skip-not-installable restriction from debian/tests/control.",
        "Drop deprecated skip-not-installable restriction.",
        actions,
    )
    .with_certainty(Certainty::Possible)])
}

declare_detector! {
    name: "testsuite-restrictions-has-deprecated-skip-not-installable",
    tags: ["testsuite-restrictions-has-deprecated-skip-not-installable"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/tests/control",
            paragraph_key: "Tests",
            field: "Restrictions",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/tests/control",
            paragraph_key: "Test-Command",
            field: "Restrictions",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_control(tmp: &TempDir, contents: &str) -> PathBuf {
        let tests = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests).unwrap();
        let path = tests.join("control");
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_drop_sole_restriction() {
        let tmp = TempDir::new().unwrap();
        let path = write_control(
            &tmp,
            "Tests: test1\nRestrictions: skip-not-installable\nDepends: @\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Possible));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Tests: test1\nDepends: @\n",
        );
    }

    #[test]
    fn test_drop_among_other_restrictions() {
        let tmp = TempDir::new().unwrap();
        let path = write_control(
            &tmp,
            "Tests: test1\nRestrictions: needs-root, skip-not-installable, allow-stderr\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Tests: test1\nRestrictions: needs-root, allow-stderr\n",
        );
    }

    #[test]
    fn test_test_command_paragraph() {
        let tmp = TempDir::new().unwrap();
        let path = write_control(
            &tmp,
            "Test-Command: ./run\nRestrictions: skip-not-installable\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "Test-Command: ./run\n",);
    }

    #[test]
    fn test_multiple_paragraphs() {
        let tmp = TempDir::new().unwrap();
        let path = write_control(
            &tmp,
            "Tests: a\nRestrictions: skip-not-installable\n\nTests: b\nDepends: @\n\nTests: c\nRestrictions: needs-root, skip-not-installable\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Tests: a\n\nTests: b\nDepends: @\n\nTests: c\nRestrictions: needs-root\n",
        );
    }

    #[test]
    fn test_no_changes_without_restriction() {
        let tmp = TempDir::new().unwrap();
        let original = "Tests: test1\nRestrictions: needs-root\n";
        let path = write_control(&tmp, original);

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_changes_without_control() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("debian")).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
