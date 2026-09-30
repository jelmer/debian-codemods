//! Fixer for the `synopsis-is-a-sentence` tag.
//!
//! lintian's `fields/description` check flags a synopsis whose first
//! sentence ends with `.` (either at the end of the synopsis or followed
//! by more words), unless the trailing token is `etc.`, `e.g.`, `eg.`
//! or `...`. This fixer strips the trailing period when doing so is
//! obviously safe (a single sentence with a terminating full stop) and
//! backs off otherwise.

use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DESCRIPTION: &str = "Description synopsis is written as a sentence.";
const LABEL: &str = "Remove trailing full stop from Description synopsis.";

/// Does `synopsis` trigger lintian's `synopsis-is-a-sentence` regex?
///
/// The lintian check is `m/(.*\.)(?:\s*$|\s+\S+)/i` with three
/// exemptions on the matched prefix: a trailing `etc.`, a trailing
/// `e.g.` (or `eg.`), or a trailing `...` not immediately preceded by
/// another dot.
fn triggers(synopsis: &str) -> bool {
    let Some(prefix) = greedy_prefix_with_period(synopsis) else {
        return false;
    };
    let after = &synopsis[prefix.len()..];
    // `(?:\s*$|\s+\S+)`: either only whitespace to the end, or at
    // least one whitespace followed by a non-whitespace character.
    let leads_with_space = after.chars().next().is_some_and(char::is_whitespace);
    let matches_tail = after.trim().is_empty() || (leads_with_space && !after.trim().is_empty());
    if !matches_tail {
        return false;
    }
    if ends_with_etc(prefix) || ends_with_eg(prefix) || ends_with_ellipsis(prefix) {
        return false;
    }
    true
}

/// Longest prefix of `s` that ends with `.` (byte-indexed), or `None`
/// when there is no period. Mirrors perl's greedy `.*\.`.
fn greedy_prefix_with_period(s: &str) -> Option<&str> {
    s.rfind('.').map(|idx| &s[..idx + 1])
}

/// True when `prefix` ends with `\s+etc.` — the whitespace is required
/// so `foo.etc.` is not treated as the "etc." exemption.
fn ends_with_etc(prefix: &str) -> bool {
    let Some(head) = prefix.strip_suffix("etc.") else {
        return false;
    };
    head.chars().next_back().is_some_and(char::is_whitespace)
}

/// True when `prefix` ends with `\s+e.g.` or `\s+eg.` (matching perl's
/// `\s+e\.?g\.$`).
fn ends_with_eg(prefix: &str) -> bool {
    for suffix in ["e.g.", "eg."] {
        let Some(head) = prefix.strip_suffix(suffix) else {
            continue;
        };
        if head.chars().next_back().is_some_and(char::is_whitespace) {
            return true;
        }
    }
    false
}

/// True when `prefix` ends with `...` and the character before is not
/// another `.` (perl's `(?<!\.)\.\.\.$`).
fn ends_with_ellipsis(prefix: &str) -> bool {
    let Some(head) = prefix.strip_suffix("...") else {
        return false;
    };
    !head.ends_with('.')
}

/// If `synopsis` triggers the tag AND can be safely fixed by removing a
/// single trailing period, return the corrected synopsis. Otherwise
/// return `None` — cases like `"First. Second."` (multiple sentences)
/// are left alone so we don't drop meaningful text.
fn strip_trailing_period(synopsis: &str) -> Option<String> {
    if !triggers(synopsis) {
        return None;
    }
    // Only rewrite when the period is at the end of the synopsis
    // (possibly followed by whitespace) and there is exactly one
    // period. Any embedded period would mean stripping the trailing
    // one still leaves the synopsis reading as a sentence.
    let trimmed = synopsis.trim_end();
    let head = trimmed.strip_suffix('.')?;
    if head.contains('.') {
        return None;
    }
    if head.trim().is_empty() {
        return None;
    }
    // Preserve any trailing whitespace that was on the synopsis line
    // (there shouldn't normally be any, but round-tripping is safe).
    let tail = &synopsis[trimmed.len()..];
    Some(format!("{head}{tail}"))
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
        let (synopsis, rest) = match description.split_once('\n') {
            Some((s, r)) => (s, Some(r)),
            None => (description.as_str(), None),
        };
        let Some(new_synopsis) = strip_trailing_period(synopsis) else {
            continue;
        };
        let Some(package_name) = binary.name() else {
            continue;
        };

        let new_description = match rest {
            Some(rest) => format!("{new_synopsis}\n{rest}"),
            None => new_synopsis,
        };

        // lintian's info is the synopsis surrounded by double quotes.
        let info = format!("\"{synopsis}\"");
        let issue = LintianIssue::binary_with_info(
            &package_name,
            "synopsis-is-a-sentence",
            Visibility::Info,
            vec![info],
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
    name: "synopsis-is-a-sentence",
    tags: ["synopsis-is-a-sentence"],
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
        adapter.apply(&ws, &FixerPreferences::default())
    }

    #[test]
    fn test_triggers_simple_trailing_period() {
        assert!(triggers("A simple tool."));
    }

    #[test]
    fn test_triggers_two_sentences() {
        // A period followed by more words also triggers the tag.
        assert!(triggers("First sentence. Second sentence"));
    }

    #[test]
    fn test_no_trigger_without_period() {
        assert!(!triggers("A simple tool"));
    }

    #[test]
    fn test_no_trigger_etc_at_end() {
        assert!(!triggers("Editor for foo, bar, etc."));
    }

    #[test]
    fn test_no_trigger_eg_at_end() {
        assert!(!triggers("Tools, e.g. for foo, bar, e.g."));
        assert!(!triggers("Editor eg."));
    }

    #[test]
    fn test_no_trigger_ellipsis() {
        assert!(!triggers("Editor for lots of things..."));
    }

    #[test]
    fn test_trigger_four_dots_is_a_sentence() {
        // Four dots: perl's `(?<!\.)\.\.\.$` requires the character
        // before the `...` to not be `.`; four dots (`....`) means the
        // character before the last `...` is `.`, so the exemption
        // does not apply and the tag is reported.
        assert!(triggers("Tool...."));
    }

    #[test]
    fn test_strip_trailing_period_simple() {
        assert_eq!(
            strip_trailing_period("A simple tool.").as_deref(),
            Some("A simple tool"),
        );
    }

    #[test]
    fn test_strip_trailing_period_backs_off_on_two_sentences() {
        // Fix would drop the second sentence — safer to leave alone.
        assert_eq!(strip_trailing_period("First. Second sentence"), None);
    }

    #[test]
    fn test_strip_trailing_period_backs_off_on_embedded_period() {
        // Removing the trailing "." would leave "e.g" in place; still
        // reads as a sentence, so refuse.
        assert_eq!(strip_trailing_period("Tool for e.g. foo."), None);
    }

    #[test]
    fn test_strip_trailing_period_none_when_not_triggered() {
        assert_eq!(strip_trailing_period("A simple tool"), None);
        assert_eq!(strip_trailing_period("Editor for foo, etc."), None);
    }

    #[test]
    fn test_fix_simple() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDescription: A simple tool.\n Extended description.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, LABEL);
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDescription: A simple tool\n Extended description.\n",
        );
    }

    #[test]
    fn test_fix_synopsis_only() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDescription: A simple tool.\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDescription: A simple tool\n",
        );
    }

    #[test]
    fn test_no_fix_two_sentences() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let original =
            "Source: test\n\nPackage: test\nDescription: First. Second sentence\n Extended.\n";
        fs::write(debian.join("control"), original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            original
        );
    }

    #[test]
    fn test_no_fix_etc_exemption() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let original =
            "Source: test\n\nPackage: test\nDescription: Editor for foo, bar, etc.\n Extended.\n";
        fs::write(debian.join("control"), original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_fix_no_period() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let original =
            "Source: test\n\nPackage: test\nDescription: command-line tool\n Extended.\n";
        fs::write(debian.join("control"), original).unwrap();

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
            "Source: test\n\nPackage: test1\nDescription: First package.\n\nPackage: test2\nDescription: Second package.\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test1\nDescription: First package\n\nPackage: test2\nDescription: Second package\n",
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
