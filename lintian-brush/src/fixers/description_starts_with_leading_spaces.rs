use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, IndentPattern, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DESCRIPTION: &str = "Description starts with leading spaces.";
const LABEL: &str = "Strip leading spaces from Description.";

/// Whether a continuation line is a bare paragraph separator.
///
/// In the value returned by `description()` the deb822 single-space
/// continuation indent has been stripped, so a separator that reads
/// `^ \.\s*$` in the control file shows up here as a `.` optionally
/// followed by whitespace. lintian skips such lines when deciding which
/// line is the "first extended line", so we do too.
fn is_bare_separator(line: &str) -> bool {
    line.strip_prefix('.')
        .is_some_and(|rest| rest.chars().all(char::is_whitespace))
}

/// Determine the lintian `info` for a `description-starts-with-leading-spaces`
/// hint, or `None` if lintian would not flag the description.
///
/// The tag fires when the first non-separator extended line starts with
/// a space followed by more whitespace in the raw control file (so at
/// least two leading spaces). After `get_multiline` strips the deb822
/// single-space indent, that shows up as a line that itself starts with
/// whitespace. Bare paragraph separators are skipped but still count
/// towards the reported 1-indexed position.
fn leading_spaces_info(description: &str) -> Option<Vec<String>> {
    let mut lines = description.split('\n');
    lines.next()?;
    for (idx, line) in lines.enumerate() {
        if is_bare_separator(line) {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            return Some(vec![format!("line {}", idx + 1)]);
        }
        return None;
    }
    None
}

/// Strip leading whitespace from the first non-separator extended line.
///
/// Later extended lines are left untouched: lintian only checks the
/// first extended line, and leading whitespace on later lines can be
/// meaningful (verbatim display).
fn strip_leading_spaces(description: &str) -> String {
    let mut out = String::with_capacity(description.len());
    let mut lines = description.split('\n');
    if let Some(synopsis) = lines.next() {
        out.push_str(synopsis);
    }
    let mut stripped = false;
    for line in lines {
        out.push('\n');
        if !stripped && !is_bare_separator(line) {
            out.push_str(line.trim_start());
            stripped = true;
        } else {
            out.push_str(line);
        }
    }
    out
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
        let Some(info) = leading_spaces_info(&description) else {
            continue;
        };
        let Some(package_name) = binary.name() else {
            continue;
        };

        let new_description = strip_leading_spaces(&description);

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "description-starts-with-leading-spaces",
            Visibility::Warning,
            info,
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                DESCRIPTION,
                LABEL,
                vec![Action::Deb822(Deb822Action::SetFieldWithIndent {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: package_name,
                    },
                    field: "Description".into(),
                    value: new_description,
                    indent: IndentPattern::Fixed { spaces: 1 },
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "description-starts-with-leading-spaces",
    tags: ["description-starts-with-leading-spaces"],
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
        assert!(!is_bare_separator(". text"));
        assert!(!is_bare_separator("   "));
        assert!(!is_bare_separator("text"));
    }

    #[test]
    fn test_leading_spaces_info_none() {
        assert_eq!(leading_spaces_info("Synopsis\nExtended line."), None);
        // Synopsis-only description: nothing to flag.
        assert_eq!(leading_spaces_info("Synopsis"), None);
    }

    #[test]
    fn test_leading_spaces_info_first_extended_flagged() {
        assert_eq!(
            leading_spaces_info("Synopsis\n Indented line."),
            Some(vec!["line 1".to_string()])
        );
    }

    #[test]
    fn test_leading_spaces_info_later_extended_not_flagged() {
        // lintian only inspects the first extended line.
        assert_eq!(
            leading_spaces_info("Synopsis\nFirst line.\n Later indented."),
            None
        );
    }

    #[test]
    fn test_leading_spaces_info_skips_bare_separator() {
        // A bare paragraph separator between synopsis and the first
        // real extended line does not stop the tag from firing, but
        // the reported line number counts it.
        assert_eq!(
            leading_spaces_info("Synopsis\n.\n Indented line."),
            Some(vec!["line 2".to_string()])
        );
    }

    #[test]
    fn test_strip_leading_spaces_first_line() {
        assert_eq!(
            strip_leading_spaces("Synopsis\n   Extra spaces.\n Regular line."),
            "Synopsis\nExtra spaces.\n Regular line."
        );
    }

    #[test]
    fn test_strip_leading_spaces_preserves_bare_separator() {
        assert_eq!(
            strip_leading_spaces("Synopsis\n.\n   Extra spaces.\n More."),
            "Synopsis\n.\nExtra spaces.\n More."
        );
    }

    #[test]
    fn test_fix_leading_spaces() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n   Extended description here.\n Second line.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, LABEL);

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n Extended description here.\n Second line.\n",
        );
    }

    #[test]
    fn test_no_leading_spaces() {
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
    fn test_only_later_extended_line_indented() {
        // Leading whitespace on later extended lines is meaningful for
        // verbatim display and must not be touched.
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let original =
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n First line.\n   Indented verbatim block.\n";
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
    fn test_multiple_packages() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test1\nDescription: First package\n   Bad first line.\n\nPackage: test2\nDescription: Second package\n Fine first line.\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test1\nDescription: First package\n Bad first line.\n\nPackage: test2\nDescription: Second package\n Fine first line.\n",
        );
    }
}
