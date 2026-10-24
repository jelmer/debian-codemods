use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, IndentPattern, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DESCRIPTION: &str = "Description contains an invalid control statement.";
const LABEL: &str = "Replace indented paragraph separators in Description.";

/// Whether a continuation line is a bare paragraph separator.
///
/// In the value returned by `description()` the deb822 single-space
/// continuation indent has been stripped, so a separator that reads
/// `^ \.\s*$` in the control file shows up here as a `.` optionally
/// followed by whitespace.
fn is_bare_separator(line: &str) -> bool {
    line.strip_prefix('.')
        .is_some_and(|rest| rest.chars().all(char::is_whitespace))
}

/// Whether a continuation line is a separator with extra leading
/// whitespace, i.e. `^ \s+\.\s*$` in the control file.
fn is_indented_separator(line: &str) -> bool {
    line.starts_with(char::is_whitespace) && is_bare_separator(line.trim_start())
}

/// Whether a continuation line has text following the dot, i.e.
/// `^ \.\s*\S` in the control file.
fn is_dot_followed_by_text(line: &str) -> bool {
    line.strip_prefix('.')
        .is_some_and(|rest| !rest.trim().is_empty())
}

#[derive(Debug, PartialEq, Eq)]
struct Hit {
    /// 1-indexed position in the extended description, as lintian
    /// reports it.
    position: usize,
    fixable: bool,
}

/// Find the extended description lines lintian flags as invalid control
/// statements.
///
/// Only indented separators are considered fixable: the intent is
/// clearly an empty line. A dot followed by text (e.g. ".NET" or
/// ". foo") may be a separator that lost its line break or text that
/// happens to start with a dot, so we can't tell what was meant.
fn find_hits(description: &str) -> Vec<Hit> {
    description
        .split('\n')
        .skip(1)
        .enumerate()
        .filter_map(|(idx, line)| {
            if is_bare_separator(line) {
                None
            } else if is_indented_separator(line) {
                Some(Hit {
                    position: idx + 1,
                    fixable: true,
                })
            } else if is_dot_followed_by_text(line) {
                Some(Hit {
                    position: idx + 1,
                    fixable: false,
                })
            } else {
                None
            }
        })
        .collect()
}

fn fix_indented_separators(description: &str) -> String {
    description
        .split('\n')
        .enumerate()
        .map(|(idx, line)| {
            if idx > 0 && is_indented_separator(line) {
                "."
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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
        let hits = find_hits(&description);
        if hits.is_empty() {
            continue;
        }
        let Some(package) = binary.name() else {
            continue;
        };
        let set_field = Action::Deb822(Deb822Action::SetFieldWithIndent {
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Binary {
                package: package.clone(),
            },
            field: "Description".into(),
            value: fix_indented_separators(&description),
            indent: IndentPattern::Fixed { spaces: 1 },
        });

        // One diagnostic per lintian hint; re-applying an identical
        // SetField is a no-op, so the tree is rewritten once while
        // every fixable tag is recorded as fixed.
        for hit in hits {
            let issue = LintianIssue::binary_with_info(
                &package,
                "description-contains-invalid-control-statement",
                Visibility::Error,
                vec![format!("line {}", hit.position)],
            );
            let diagnostic = if hit.fixable {
                Diagnostic::with_actions(issue, DESCRIPTION, LABEL, vec![set_field.clone()])
                    .with_certainty(Certainty::Likely)
            } else {
                Diagnostic::with_plans(issue, DESCRIPTION, vec![])
            };
            diagnostics.push(diagnostic);
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "description-contains-invalid-control-statement",
    tags: ["description-contains-invalid-control-statement"],
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
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        adapter.apply(
            &ws,
            &FixerPreferences {
                minimum_certainty: Some(Certainty::Likely),
                ..Default::default()
            },
        )
    }

    #[test]
    fn test_is_indented_separator() {
        assert!(is_indented_separator(" ."));
        assert!(is_indented_separator("   . "));
        assert!(is_indented_separator("\t."));
        assert!(!is_indented_separator("."));
        assert!(!is_indented_separator(" . text"));
        assert!(!is_indented_separator(" text."));
    }

    #[test]
    fn test_is_dot_followed_by_text() {
        assert!(is_dot_followed_by_text(".NET"));
        assert!(is_dot_followed_by_text(". text"));
        assert!(!is_dot_followed_by_text("."));
        assert!(!is_dot_followed_by_text(". "));
        assert!(!is_dot_followed_by_text(" .NET"));
        assert!(!is_dot_followed_by_text("text."));
    }

    #[test]
    fn test_find_hits_none() {
        assert_eq!(find_hits("Synopsis"), vec![]);
        assert_eq!(find_hits("Synopsis\nFirst.\n.\nSecond."), vec![]);
        // The synopsis is not checked.
        assert_eq!(find_hits(".synopsis\nFirst."), vec![]);
    }

    #[test]
    fn test_find_hits_counts_separators() {
        assert_eq!(
            find_hits("Synopsis\nFirst.\n.\nSecond.\n  .\n.NET"),
            vec![
                Hit {
                    position: 4,
                    fixable: true,
                },
                Hit {
                    position: 5,
                    fixable: false,
                },
            ]
        );
    }

    #[test]
    fn test_fix_indented_separators() {
        assert_eq!(
            fix_indented_separators("Synopsis\nFirst.\n  .\nSecond.\n .\n.NET"),
            "Synopsis\nFirst.\n.\nSecond.\n.\n.NET"
        );
    }

    #[test]
    fn test_fix() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n First paragraph.\n   .\n Second paragraph.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, LABEL);
        assert_eq!(result.certainty, Some(Certainty::Likely));

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n First paragraph.\n .\n Second paragraph.\n",
        );
    }

    #[test]
    fn test_fix_preserves_verbatim_indent() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n Features:\n   * one\n  .\n   * two\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n Features:\n   * one\n .\n   * two\n",
        );
    }

    #[test]
    fn test_dot_followed_by_text_not_fixed() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let original = "Source: test\n\nPackage: test\nDescription: A tool for testing\n Bindings for\n .NET applications.\n";
        fs::write(debian.join("control"), original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            original
        );
    }

    #[test]
    fn test_not_certain_enough() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let original =
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n First.\n  .\n Second.\n";
        fs::write(debian.join("control"), original).unwrap();

        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("test".into()),
            Some("1.0".parse().unwrap()),
        );
        let result = DetectorImpl.apply(
            &ws,
            &FixerPreferences {
                minimum_certainty: Some(Certainty::Certain),
                ..Default::default()
            },
        );
        assert!(matches!(result, Err(FixerError::NotCertainEnough(..))));
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
            "Source: test\n\nPackage: test1\nDescription: First package\n Text.\n  .\n More.\n\nPackage: test2\nDescription: Second package\n Text.\n .\n More.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test1\nDescription: First package\n Text.\n .\n More.\n\nPackage: test2\nDescription: Second package\n Text.\n .\n More.\n",
        );
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
