//! Fixer for `odd-mark-in-description`.
//!
//! lintian's `fields/description` check emits this pedantic tag when the
//! synopsis or any line of the extended description contains a comma
//! that is immediately followed by a character that is neither
//! whitespace nor a digit (regex `,[^\s\d]`). Digits are excluded so
//! grouped numbers like `300,000` are not flagged. This fixer inserts a
//! single space after each offending comma.

use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DESCRIPTION: &str = "Description contains a comma not followed by whitespace.";
const LABEL: &str = "Add missing whitespace after comma in Description.";

/// Returns true if `s` has a comma immediately followed by a character
/// that is neither whitespace nor an ASCII digit. Mirrors lintian's
/// `/,[^\s\d]/` on the Perl side.
fn has_odd_comma(s: &str) -> bool {
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b',' {
            continue;
        }
        let Some(&next) = bytes.get(i + 1) else {
            continue;
        };
        let next_char = next as char;
        if next_char.is_ascii_digit() || next_char.is_whitespace() {
            continue;
        }
        // Multi-byte UTF-8 continuation bytes are 0x80..=0xBF, i.e. not
        // ASCII whitespace or digits, so any such byte triggers the tag
        // just as it would in lintian.
        return true;
    }
    false
}

/// Insert a single space after every comma matched by [`has_odd_comma`].
///
/// Comma is ASCII and always a single UTF-8 byte, so the byte offsets
/// we insert at are guaranteed to fall on a code-point boundary.
fn fix_odd_commas(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    for (i, &b) in bytes.iter().enumerate() {
        out.push(b);
        if b != b',' {
            continue;
        }
        let Some(&next) = bytes.get(i + 1) else {
            continue;
        };
        // A UTF-8 continuation byte (0x80..=0xBF) is neither ASCII
        // whitespace nor an ASCII digit, so it falls into the
        // "needs fix" branch — matching lintian's `[^\s\d]` which
        // treats every non-ASCII character as a non-digit non-space.
        let next_char = next as char;
        if !next_char.is_ascii_digit() && !next_char.is_whitespace() {
            out.push(b' ');
        }
    }
    // Safe: we only ever appended existing bytes from `s` and ASCII
    // spaces, so the result is valid UTF-8.
    String::from_utf8(out).expect("UTF-8 preserved")
}

/// A single odd-mark hit, with the lintian `info` lintian would emit
/// for it and the (synopsis vs. extended) context needed to rewrite.
struct Hit {
    /// `comma not followed by whitespace (synopsis)` or
    /// `comma not followed by whitespace (line N)`.
    info: String,
}

/// Collect every odd-mark hit lintian would emit for `description`, in
/// emission order (synopsis first, then extended lines).
///
/// The synopsis is the text before the first `\n`; everything after is
/// the extended description. lintian iterates the extended lines with a
/// 1-indexed counter that increments on *every* iteration, including
/// bare paragraph separators that it skips, so we do the same.
fn find_hits(description: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let (synopsis, extended) = match description.split_once('\n') {
        Some((s, e)) => (s, Some(e)),
        None => (description, None),
    };
    if has_odd_comma(synopsis) {
        hits.push(Hit {
            info: "comma not followed by whitespace (synopsis)".to_string(),
        });
    }
    if let Some(extended) = extended {
        for (idx, line) in extended.split('\n').enumerate() {
            // Bare paragraph separators (`.` optionally followed by
            // whitespace, after deb822 continuation stripping) are
            // skipped by lintian but still occupy a line number.
            if line
                .strip_prefix('.')
                .is_some_and(|rest| rest.chars().all(char::is_whitespace))
            {
                continue;
            }
            if has_odd_comma(line) {
                hits.push(Hit {
                    info: format!("comma not followed by whitespace (line {})", idx + 1),
                });
            }
        }
    }
    hits
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
        let new_description = fix_odd_commas(&description);
        let set_field = Action::Deb822(Deb822Action::SetField {
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Binary {
                package: package.clone(),
            },
            field: "Description".into(),
            value: new_description,
        });

        // One diagnostic per lintian hint; re-applying an identical
        // SetField is a no-op, so the tree is rewritten once while
        // every tag is recorded as fixed.
        for hit in hits {
            let issue = LintianIssue::binary_with_info(
                &package,
                "odd-mark-in-description",
                Visibility::Pedantic,
                vec![hit.info],
            );
            diagnostics.push(
                Diagnostic::with_actions(issue, DESCRIPTION, LABEL, vec![set_field.clone()])
                    .with_certainty(Certainty::Possible),
            );
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "odd-mark-in-description",
    tags: ["odd-mark-in-description"],
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

    #[test]
    fn test_has_odd_comma_hits() {
        assert!(has_odd_comma("foo,bar"));
        assert!(has_odd_comma("one,two, three"));
        // Non-ASCII character after the comma is still an oddity.
        assert!(has_odd_comma("foo,é"));
    }

    #[test]
    fn test_has_odd_comma_misses() {
        assert!(!has_odd_comma("foo, bar"));
        assert!(!has_odd_comma("300,000 units"));
        assert!(!has_odd_comma("no commas here"));
        // Trailing comma with nothing after it isn't flagged by lintian.
        assert!(!has_odd_comma("trailing,"));
        // Comma followed by newline is whitespace, so fine.
        assert!(!has_odd_comma("foo,\n bar"));
    }

    #[test]
    fn test_fix_odd_commas_inserts_space() {
        assert_eq!(fix_odd_commas("foo,bar"), "foo, bar");
        assert_eq!(fix_odd_commas("a,b,c"), "a, b, c");
    }

    #[test]
    fn test_fix_odd_commas_preserves_grouped_numbers() {
        assert_eq!(fix_odd_commas("300,000 units"), "300,000 units");
    }

    #[test]
    fn test_fix_odd_commas_leaves_proper_spacing() {
        assert_eq!(fix_odd_commas("foo, bar"), "foo, bar");
        assert_eq!(fix_odd_commas("trailing,"), "trailing,");
    }

    #[test]
    fn test_find_hits_synopsis() {
        let hits = find_hits("tool,for things\n Extended line.");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].info, "comma not followed by whitespace (synopsis)");
    }

    #[test]
    fn test_find_hits_extended_line_number() {
        // lintian's $position starts at 1 for the first extended line.
        let hits = find_hits("clean synopsis\nFirst line.\nSecond,line.");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].info, "comma not followed by whitespace (line 2)");
    }

    #[test]
    fn test_find_hits_bare_separator_still_counts() {
        // The bare `.` paragraph separator is skipped by lintian but
        // still advances the position counter, so the following line
        // is reported as line 3 even though only two "real" lines
        // precede it.
        let hits = find_hits("clean synopsis\nFirst line.\n.\nSecond,line.");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].info, "comma not followed by whitespace (line 3)");
    }

    #[test]
    fn test_find_hits_multiple() {
        let hits = find_hits("bad,synopsis\nAlso,bad.\n More,here.");
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].info, "comma not followed by whitespace (synopsis)");
        assert_eq!(hits[1].info, "comma not followed by whitespace (line 1)");
        assert_eq!(hits[2].info, "comma not followed by whitespace (line 2)");
    }

    #[test]
    fn test_find_hits_none() {
        assert!(find_hits("clean synopsis\n Extended line, with spaces.").is_empty());
    }

    #[test]
    fn test_fix_extended_description() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDescription: A test package\n Supports foo,bar,baz formats.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, LABEL);
        assert_eq!(result.certainty, Some(Certainty::Possible));
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDescription: A test package\n Supports foo, bar, baz formats.\n",
        );
    }

    #[test]
    fn test_fix_synopsis() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDescription: tool,for everything\n Extended line.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        let tags = result.fixed_lintian_tags();
        assert_eq!(tags, vec!["odd-mark-in-description"]);
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDescription: tool, for everything\n Extended line.\n",
        );
    }

    #[test]
    fn test_grouped_number_not_fixed() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let original =
            "Source: test\n\nPackage: test\nDescription: tool for data\n Handles up to 300,000 records per batch.\n";
        fs::write(debian.join("control"), original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_clean() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let original =
            "Source: test\n\nPackage: test\nDescription: A test package\n A clean extended line.\n";
        fs::write(debian.join("control"), original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
