use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DESCRIPTION: &str = "Description contains tab characters.";
const LABEL: &str = "Replace tabs with spaces in Description.";

/// Whether a continuation line is a bare paragraph separator.
///
/// In the value returned by `description()` the deb822 continuation
/// indicator has been stripped, so a separator that reads `^ \.\s*$` in
/// the control file shows up here as a `.` optionally followed by
/// whitespace. lintian's tab check skips such lines, so we do too.
fn is_bare_separator(line: &str) -> bool {
    line.strip_prefix('.')
        .is_some_and(|rest| rest.chars().all(char::is_whitespace))
}

/// Determine the lintian `info` for a `description-contains-tabs` hint,
/// or `None` if lintian would not flag the description at all.
///
/// lintian emits the tag at most once per binary package. When the
/// synopsis (the first line) contains a tab the hint carries no info;
/// otherwise it points at the first continuation line that contains a
/// tab with a 1-indexed `"line N"`. Bare paragraph separators are
/// skipped but still count towards the line number, matching lintian.
fn tab_issue_info(description: &str) -> Option<Vec<String>> {
    let mut lines = description.split('\n');
    let synopsis = lines.next().unwrap_or("");
    if synopsis.contains('\t') {
        return Some(vec![]);
    }
    for (idx, line) in lines.enumerate() {
        if is_bare_separator(line) {
            continue;
        }
        if line.contains('\t') {
            return Some(vec![format!("line {}", idx + 1)]);
        }
    }
    None
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
        let Some(description) = binary.description() else {
            continue;
        };
        let Some(info) = tab_issue_info(&description) else {
            continue;
        };
        let Some(package_name) = binary.name() else {
            continue;
        };

        let new_description = description.replace('\t', " ");

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "description-contains-tabs",
            Visibility::Error,
            info,
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                DESCRIPTION,
                LABEL,
                vec![Action::Deb822(Deb822Action::SetField {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: package_name,
                    },
                    field: "Description".into(),
                    value: new_description,
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "description-contains-tabs",
    tags: ["description-contains-tabs"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Description",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

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

    #[test]
    fn test_is_bare_separator() {
        assert!(is_bare_separator("."));
        assert!(is_bare_separator(". "));
        assert!(is_bare_separator(".\t"));
        assert!(!is_bare_separator(". text"));
        assert!(!is_bare_separator("   "));
        assert!(!is_bare_separator("text"));
    }

    #[test]
    fn test_tab_issue_info_synopsis() {
        assert_eq!(
            tab_issue_info("A\ttabbed synopsis\n extended"),
            Some(vec![])
        );
    }

    #[test]
    fn test_tab_issue_info_extended() {
        assert_eq!(
            tab_issue_info("Plain synopsis\nFirst line.\nSecond\tline."),
            Some(vec!["line 2".to_string()])
        );
    }

    #[test]
    fn test_tab_issue_info_skips_bare_separator() {
        // A tab on a bare paragraph separator is not flagged by lintian,
        // but the line still counts: the next real line with a tab is
        // reported with its 1-indexed position.
        assert_eq!(
            tab_issue_info("Plain synopsis\nFirst line.\n.\t\nReal\ttab."),
            Some(vec!["line 3".to_string()])
        );
    }

    #[test]
    fn test_tab_issue_info_none() {
        assert_eq!(tab_issue_info("Plain synopsis\n extended line."), None);
        // Tab present only on a bare separator: lintian would not flag it.
        assert_eq!(tab_issue_info("Plain synopsis\n.\t"), None);
    }

    #[test]
    fn test_fix_tab_in_synopsis() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDescription: A\ttool\tfor testing\n Extended description here.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, LABEL);

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n Extended description here.\n",
        );
    }

    #[test]
    fn test_fix_tab_in_extended_description() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n First line.\n Second\tline.\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n First line.\n Second line.\n",
        );
    }

    #[test]
    fn test_no_tabs() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let original =
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n Extended.\n";
        fs::write(debian.join("control"), original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            original
        );
    }

    #[test]
    fn test_multiple_packages() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test1\nDescription: First\tpackage\n\nPackage: test2\nDescription: Second package\n Extended\tline.\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test1\nDescription: First package\n\nPackage: test2\nDescription: Second package\n Extended line.\n",
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
}
