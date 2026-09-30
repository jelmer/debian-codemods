use crate::declare_detector;
use crate::diagnostic::{
    Action, ActionPlan, Deb822Action, Diagnostic, IndentPattern, ParagraphSelector,
};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Whether `pattern` matches any shipped file, or `None` when the
/// pattern isn't a valid DEP-5 glob.
///
/// A bad escape sequence is lintian's `invalid-escape-sequence-in-dep5-copyright`,
/// and lintian skips such a pattern rather than judging it superfluous;
/// `None` makes us skip it too.
fn pattern_matches_any(pattern: &str, names: &[String]) -> Option<bool> {
    let glob = debian_copyright::GlobPattern::try_new(pattern).ok()?;
    Some(names.iter().any(|name| glob.is_match(name)))
}

/// Drop `pattern` from a `Files:` value, keeping the line structure of
/// what remains so the diff stays limited to the pattern itself.
///
/// Returns `None` if removing it would leave a line -- and hence the
/// whole field -- empty.
fn remove_pattern(value: &str, pattern: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in value.lines() {
        let kept: Vec<&str> = line.split_whitespace().filter(|p| *p != pattern).collect();
        if kept.is_empty() {
            continue;
        }
        lines.push(kept.join(" "));
    }
    if lines.is_empty() {
        return None;
    }
    Some(lines.join("\n"))
}

/// Collect the source files lintian would match patterns against.
///
/// Lintian looks at the orig tarball plus the patched `debian/`
/// directory; we only have the unpacked tree. The tree may hold files
/// the tarball doesn't (build artifacts, VCS metadata), which can only
/// make a pattern look *less* superfluous -- the safe direction.
///
/// Directories that never appear in an orig tarball are skipped so that
/// a pattern only matching, say, `.git/` isn't considered live.
fn shipped_names(ws: &dyn Workspace) -> Result<Option<Vec<String>>, FixerError> {
    const SKIP_TOP_LEVEL: &[&str] = &[".pc", ".git", ".bzr", ".hg", ".svn"];

    let Some(paths) = ws.walk_dir(Path::new("."))? else {
        return Ok(None);
    };

    let mut names = Vec::new();
    for path in paths {
        // walk_dir yields paths prefixed with the "." we passed in.
        let rel = path.strip_prefix(".").unwrap_or(&path);
        let Some(name) = rel.to_str() else {
            // A non-UTF-8 name we can't reason about; bail out rather
            // than silently pretend the tree is smaller than it is.
            return Ok(None);
        };
        if SKIP_TOP_LEVEL
            .iter()
            .any(|d| name == *d || name.starts_with(&format!("{}/", d)))
        {
            continue;
        }
        names.push(name.to_string());
    }
    Ok(Some(names))
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let copyright = match ws.parsed_copyright() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };

    let Some(names) = shipped_names(ws)? else {
        // Host can't walk the tree (e.g. an LSP serving open buffers).
        return Ok(Vec::new());
    };
    // An empty tree means we're not looking at a real source package;
    // every pattern would look superfluous.
    if names.is_empty() {
        return Ok(Vec::new());
    }

    // Lintian only evaluates globbing when commas can't be legal
    // separators, i.e. unless some Files field uses a comma while no
    // shipped file has one in its name.
    let names_with_comma = names.iter().any(|n| n.contains(','));
    let fields_with_comma = copyright
        .iter_files()
        .filter_map(|fp| fp.as_deb822().get("Files"))
        .any(|v| v.contains(','));
    if fields_with_comma && !names_with_comma {
        return Ok(Vec::new());
    }

    // A pattern used by more than one paragraph is reported as
    // duplicate-globbing-patterns instead, and one that is redundant
    // within its own paragraph as redundant-globbing-patterns. Neither
    // is ours to fix, and lintian explicitly suppresses
    // superfluous-file-pattern for them.
    let mut seen = HashSet::new();
    let mut duplicated = HashSet::new();
    for fp in copyright.iter_files() {
        let mut in_paragraph = HashSet::new();
        for pattern in fp.files() {
            if !in_paragraph.insert(pattern.clone()) {
                duplicated.insert(pattern.clone());
            }
            if !seen.insert(pattern.clone()) {
                duplicated.insert(pattern);
            }
        }
    }

    let mut diagnostics = Vec::new();

    for fp in copyright.iter_files() {
        let paragraph = fp.as_deb822();
        let Some(files) = paragraph.get("Files") else {
            continue;
        };
        let patterns = fp.files();

        let mut kept = Vec::new();
        let mut dropped = Vec::new();
        for pattern in &patterns {
            let superfluous = !duplicated.contains(pattern)
                && matches!(pattern_matches_any(pattern, &names), Some(false));
            if superfluous {
                dropped.push(pattern.clone());
            } else {
                kept.push(pattern.clone());
            }
        }

        if dropped.is_empty() {
            continue;
        }

        // Two cases get reported but not fixed:
        //
        // * Every pattern in the paragraph is dead. Dropping them all
        //   would leave a Files paragraph with no Files field, and
        //   removing the paragraph outright discards copyright and
        //   licensing information that may still be wanted.
        // * Several patterns in one paragraph are dead. Each is a
        //   separate lintian tag that overrides can silence
        //   individually, but the fix rewrites the whole Files field --
        //   so honouring an override on one of them would mean emitting
        //   a different rewrite than the one planned here.
        let action = if kept.is_empty() || dropped.len() > 1 {
            None
        } else {
            remove_pattern(&files, &dropped[0]).map(|value| {
                Action::Deb822(Deb822Action::SetFieldWithIndent {
                    file: copyright_rel.clone(),
                    paragraph: ParagraphSelector::CopyrightFiles {
                        glob: files.clone(),
                    },
                    field: "Files".into(),
                    value,
                    indent: IndentPattern::Fixed { spaces: 1 },
                })
            })
        };

        let line_number = paragraph
            .entries()
            .find(|e| e.key().as_deref() == Some("Files"))
            .map(|e| e.line() + 1)
            .unwrap_or_else(|| paragraph.line() + 1);

        for pattern in dropped {
            let issue = LintianIssue::source_with_info(
                "superfluous-file-pattern",
                Visibility::Warning,
                vec![format!("{} [debian/copyright:{}]", pattern, line_number)],
            );
            let diagnostic = Diagnostic::with_actions(
                issue,
                format!("pattern\t{}", pattern),
                format!("Remove superfluous file pattern {}.", pattern),
                action.clone().into_iter().collect(),
            );
            // The unpacked tree isn't byte-for-byte the orig tarball
            // lintian inspects, so "matches nothing" is a strong hint
            // rather than a certainty.
            diagnostics.push(diagnostic.with_certainty(Certainty::Likely));
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let patterns: Vec<&str> = fixed
        .iter()
        .filter_map(|(d, _)| d.message.strip_prefix("pattern\t"))
        .collect();
    format!(
        "Remove superfluous file pattern{} {} from debian/copyright.",
        if patterns.len() == 1 { "" } else { "s" },
        patterns.join(", ")
    )
}

declare_detector! {
    name: "superfluous-file-pattern",
    tags: ["superfluous-file-pattern"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Files",
            field: "Files",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
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

    fn matches(pattern: &str, name: &str) -> bool {
        pattern_matches_any(pattern, &[name.to_string()]).unwrap()
    }

    #[test]
    fn test_star_matches_slashes_and_leading_dots() {
        assert!(matches("*", "src/foo.c"));
        assert!(matches("*", ".hidden"));
        assert!(matches("src/*", "src/deep/foo.c"));
        assert!(!matches("src/*", "other/foo.c"));
    }

    #[test]
    fn test_remove_pattern_keeps_line_structure() {
        assert_eq!(
            remove_pattern("a.c b.c\nc.c d.c", "c.c").unwrap(),
            "a.c b.c\nd.c"
        );
        // A line that held nothing else disappears rather than being
        // left blank.
        assert_eq!(remove_pattern("a.c b.c\nc.c", "c.c").unwrap(), "a.c b.c");
    }

    #[test]
    fn test_remove_pattern_declines_to_empty_the_field() {
        assert_eq!(remove_pattern("a.c", "a.c"), None);
    }

    #[test]
    fn test_question_mark_matches_single_character() {
        assert!(matches("a?c", "abc"));
        assert!(!matches("a?c", "abbc"));
    }

    #[test]
    fn test_metacharacters_are_literal() {
        // Unlike a shell glob, brackets and braces have no meaning.
        assert!(matches("a[bc]d", "a[bc]d"));
        assert!(!matches("a[bc]d", "abd"));
        assert!(matches("{x,y}", "{x,y}"));
        assert!(!matches("foo.c", "fooxc"));
    }

    #[test]
    fn test_pattern_is_anchored() {
        assert!(!matches("foo", "foo.c"));
        assert!(!matches("foo.c", "src/foo.c"));
    }

    #[test]
    fn test_escaped_wildcard_is_a_literal() {
        assert!(matches(r"a\*b", "a*b"));
        assert!(!matches(r"a\*b", "axb"));
    }

    #[test]
    fn test_invalid_escape_sequence_is_skipped() {
        // lintian reports these as invalid-escape-sequence-in-dep5-copyright
        // and skips the pattern rather than calling it superfluous.
        assert_eq!(pattern_matches_any(r"a\xb", &["a\\xb".to_string()]), None);
        assert_eq!(pattern_matches_any("trailing\\", &[]), None);
    }

    fn make_tree(tmp: &Path, files: &[(&str, &str)]) {
        for (path, content) in files {
            let full = tmp.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, content).unwrap();
        }
    }

    const HEADER: &str =
        "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n";

    #[test]
    fn test_drop_dead_pattern() {
        let tmp = TempDir::new().unwrap();
        make_tree(
            tmp.path(),
            &[
                ("src/foo.c", "int main(void) { return 0; }\n"),
                ("debian/changelog", "test (1.0) unstable; urgency=low\n"),
                (
                    "debian/copyright",
                    &format!(
                        "{}Files: src/*\n man/*\nCopyright: 2020 Someone\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
                        HEADER
                    ),
                ),
            ],
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove superfluous file pattern man/* from debian/copyright."
        );
        assert_eq!(result.certainty, Some(Certainty::Likely));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/copyright")).unwrap(),
            format!(
                "{}Files: src/*\nCopyright: 2020 Someone\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
                HEADER
            )
        );
    }

    #[test]
    fn test_no_changes_when_all_patterns_match() {
        let tmp = TempDir::new().unwrap();
        let copyright = format!(
            "{}Files: *\nCopyright: 2020 Someone\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
            HEADER
        );
        make_tree(
            tmp.path(),
            &[("src/foo.c", "x\n"), ("debian/copyright", &copyright)],
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/copyright")).unwrap(),
            copyright
        );
    }

    #[test]
    fn test_no_fix_when_paragraph_would_be_left_empty() {
        let tmp = TempDir::new().unwrap();
        let copyright = format!(
            "{}Files: *\nCopyright: 2020 Someone\nLicense: GPL-2\n\nFiles: man/*\nCopyright: 2020 Other\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
            HEADER
        );
        make_tree(
            tmp.path(),
            &[("src/foo.c", "x\n"), ("debian/copyright", &copyright)],
        );

        // man/* is dead but it is the only pattern in its paragraph, so
        // there is nothing safe to do.
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/copyright")).unwrap(),
            copyright
        );
    }

    #[test]
    fn test_several_dead_patterns_in_one_paragraph_reported_not_fixed() {
        let tmp = TempDir::new().unwrap();
        let copyright = format!(
            "{}Files: src/* man/* doc/*\nCopyright: 2020 Somebody\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
            HEADER
        );
        make_tree(
            tmp.path(),
            &[("src/foo.c", "x\n"), ("debian/copyright", &copyright)],
        );

        // Both man/* and doc/* are dead. Each is separately overridable,
        // so rewriting the field to drop both at once isn't safe.
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/copyright")).unwrap(),
            copyright
        );
    }

    #[test]
    fn test_removal_preserves_remaining_lines() {
        let tmp = TempDir::new().unwrap();
        make_tree(
            tmp.path(),
            &[
                ("src/a.c", "x\n"),
                ("src/b.c", "x\n"),
                (
                    "debian/copyright",
                    &format!(
                        "{}Files: src/a.c src/b.c\n man/*\nCopyright: 2020 Somebody\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
                        HEADER
                    ),
                ),
            ],
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/copyright")).unwrap(),
            format!(
                "{}Files: src/a.c src/b.c\nCopyright: 2020 Somebody\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
                HEADER
            )
        );
    }

    #[test]
    fn test_duplicate_pattern_left_alone() {
        let tmp = TempDir::new().unwrap();
        let copyright = format!(
            "{}Files: src/* man/*\nCopyright: 2020 Someone\nLicense: GPL-2\n\nFiles: man/* src/*\nCopyright: 2020 Other\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
            HEADER
        );
        make_tree(
            tmp.path(),
            &[("src/foo.c", "x\n"), ("debian/copyright", &copyright)],
        );

        // man/* matches nothing, but it is used twice -- lintian reports
        // duplicate-globbing-patterns for it instead.
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/copyright")).unwrap(),
            copyright
        );
    }

    #[test]
    fn test_comma_separated_files_left_alone() {
        let tmp = TempDir::new().unwrap();
        let copyright = format!(
            "{}Files: src/*, man/*\nCopyright: 2020 Someone\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
            HEADER
        );
        make_tree(
            tmp.path(),
            &[("src/foo.c", "x\n"), ("debian/copyright", &copyright)],
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_pc_directory_ignored() {
        let tmp = TempDir::new().unwrap();
        let copyright = format!(
            "{}Files: src/* .pc/*\nCopyright: 2020 Someone\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
            HEADER
        );
        make_tree(
            tmp.path(),
            &[
                ("src/foo.c", "x\n"),
                (".pc/applied-patches", "\n"),
                ("debian/copyright", &copyright),
            ],
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove superfluous file pattern .pc/* from debian/copyright."
        );
    }

    #[test]
    fn test_not_machine_readable() {
        let tmp = TempDir::new().unwrap();
        make_tree(
            tmp.path(),
            &[
                ("src/foo.c", "x\n"),
                ("debian/copyright", "Not machine readable.\n"),
            ],
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        make_tree(tmp.path(), &[("src/foo.c", "x\n")]);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
