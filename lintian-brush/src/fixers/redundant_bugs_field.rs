use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// Whether `value` is the default Bugs value, matching lintian's
/// `m{^debbugs://bugs.debian.org/?$}i` test on the unfolded field value: the
/// default BTS URL, case-insensitive, with an optional trailing slash.
fn is_default_bugs(value: &str) -> bool {
    let value = value.trim();
    let stripped = value.strip_suffix('/').unwrap_or(value);
    stripped.eq_ignore_ascii_case("debbugs://bugs.debian.org")
}

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

    if !is_default_bugs(&bugs) {
        return Ok(Vec::new());
    }

    Ok(vec![Diagnostic::with_actions(
        LintianIssue::source("redundant-bugs-field", Visibility::Warning),
        "Source stanza sets Bugs to the default BTS URL.",
        "Remove redundant Bugs field from source stanza.",
        vec![Action::Deb822(Deb822Action::RemoveField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Bugs".into(),
        })],
    )])
}

declare_detector! {
    name: "redundant-bugs-field",
    tags: ["redundant-bugs-field"],
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
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = FsWorkspace::new(base, Some("test".into()), Some(version));
        adapter.apply(&ws, &FixerPreferences::default())
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
    fn test_is_default_bugs() {
        assert!(is_default_bugs("debbugs://bugs.debian.org"));
        assert!(is_default_bugs("debbugs://bugs.debian.org/"));
        assert!(is_default_bugs("DEBBUGS://BUGS.DEBIAN.ORG"));
        assert!(is_default_bugs("  debbugs://bugs.debian.org/  "));
        assert!(!is_default_bugs("debbugs://bugs.debian.org/foo"));
        assert!(!is_default_bugs("https://bugs.debian.org"));
        assert!(!is_default_bugs("https://github.com/example/foo/issues"));
        assert!(!is_default_bugs(""));
    }

    #[test]
    fn test_removes_redundant_bugs() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBugs: debbugs://bugs.debian.org/\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Remove redundant Bugs field from source stanza."
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_removes_redundant_bugs_without_trailing_slash() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBugs: debbugs://bugs.debian.org\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
        assert_eq!(result.fixed_lintian_issues.len(), 1);
    }

    #[test]
    fn test_keeps_non_default_bugs() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let content =
            "Source: foo\nBugs: https://github.com/example/foo/issues\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            content
        );
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_no_bugs_field() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDescription: Foo\n bar\n",
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
    fn test_diagnostic_carries_correct_info() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBugs: debbugs://bugs.debian.org/\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let diags = detect_in(base).unwrap();
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert_eq!(issue.tag.as_deref(), Some("redundant-bugs-field"));
        assert_eq!(issue.visibility, Some(Visibility::Warning));
        assert_eq!(issue.info, None);
    }

    #[test]
    fn test_overridden_diagnostic() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBugs: debbugs://bugs.debian.org/\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
        let source_dir = base.join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join("lintian-overrides"),
            "redundant-bugs-field\n",
        )
        .unwrap();

        match run_apply(base).unwrap_err() {
            FixerError::NoChangesAfterOverrides(issues) => {
                assert_eq!(issues.len(), 1);
                assert_eq!(issues[0].tag.as_deref(), Some("redundant-bugs-field"));
            }
            other => panic!("expected NoChangesAfterOverrides, got {:?}", other),
        }
        // Control file must be unchanged.
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nBugs: debbugs://bugs.debian.org/\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
    }
}
