//! Fixer for `capitalization-in-override-comment`.
//!
//! lintian's `debian/lintian-overrides/comments` check runs
//! `check_spelling_picky` over the justification (the concatenated comment
//! lines immediately preceding an override) of every override, and emits
//! the tag for each unique word in `data/spelling/corrections-case`
//! (e.g. `linux` -> `Linux`). This fixer mirrors that logic and rewrites
//! the offending words inside the comment lines.

use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, LintianOverridesAction, OverrideLineSelector};
use crate::lintian_overrides::{override_files, LintianOverrides};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

include!(concat!(env!("OUT_DIR"), "/spelling_corrections_case.rs"));

/// Return the byte range of the "core" word inside a whitespace-delimited
/// token, matching lintian's `s/^\(|[).,?!:;]+$//g`. Byte-safe because all
/// punctuation involved is ASCII.
fn core_range(text: &str, token: Range<usize>) -> Range<usize> {
    let mut start = token.start;
    if text[token.clone()].starts_with('(') {
        start += 1;
    }
    let trimmed = text[start..token.end].trim_end_matches([')', '.', ',', '?', '!', ':', ';']);
    start..start + trimmed.len()
}

/// Whitespace-delimited token byte ranges within `text`.
fn word_tokens(text: &str) -> Vec<Range<usize>> {
    let mut tokens = Vec::new();
    let mut start: Option<usize> = None;
    for (idx, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some(s) = start.take() {
                tokens.push(s..idx);
            }
        } else if start.is_none() {
            start = Some(idx);
        }
    }
    if let Some(s) = start {
        tokens.push(s..text.len());
    }
    tokens
}

/// Rewrite every picky-check misspelling in `body` using `map`. Returns the
/// new body and the list of unique `(word, correction)` pairs actually
/// applied, in first-seen order. Empty pair list means no change.
fn rewrite_body(body: &str, map: &HashMap<&str, &str>) -> (String, Vec<(String, String)>) {
    // Collect all edits (byte range + replacement + original word) first,
    // then splice right-to-left so earlier offsets stay valid.
    let mut edits: Vec<(Range<usize>, String, String)> = Vec::new();

    // `meta package` matches on the raw body, before tokenisation.
    for m in lazy_regex::regex!(r"meta\s+package").find_iter(body) {
        edits.push((
            m.start()..m.end(),
            "metapackage".to_string(),
            "meta package".to_string(),
        ));
    }

    for token in word_tokens(body) {
        let core = core_range(body, token);
        if core.is_empty() {
            continue;
        }
        let word = &body[core.clone()];
        if let Some(&correction) = map.get(word) {
            edits.push((core, correction.to_string(), word.to_string()));
        }
    }

    if edits.is_empty() {
        return (body.to_string(), Vec::new());
    }

    // Track unique words in first-seen (left-to-right) order for the caller.
    let mut seen = HashSet::new();
    let mut pairs = Vec::new();
    for (_, replacement, original) in &edits {
        if seen.insert(original.clone()) {
            pairs.push((original.clone(), replacement.clone()));
        }
    }

    // Right-to-left splice.
    let mut ordered = edits;
    ordered.sort_by_key(|(range, _, _)| std::cmp::Reverse(range.start));
    let mut new_body = body.to_string();
    for (span, replacement, _) in ordered {
        new_body.replace_range(span, &replacement);
    }
    (new_body, pairs)
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let map: HashMap<&str, &str> = SPELLING_CORRECTIONS_CASE.iter().copied().collect();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for rel in override_files(ws)? {
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let parsed = LintianOverrides::parse(content);
        let Ok(overrides) = parsed.ok() else {
            continue;
        };

        for line in overrides.lines() {
            if line.is_comment() || line.is_empty() {
                continue;
            }
            let Some(tag_token) = line.tag() else {
                continue;
            };
            let comments = line.preceding_comments();
            if comments.is_empty() {
                continue;
            }

            // Rewrite each comment body independently. A misspelling that
            // only shows up in the joined text (e.g. `meta package`
            // straddling two comment lines) is not fixable this way, so
            // we skip the diagnostic entirely.
            let mut new_bodies: Vec<String> = Vec::with_capacity(comments.len());
            let mut per_body_fixes: Vec<Vec<(String, String)>> = Vec::with_capacity(comments.len());
            for comment in &comments {
                let (new_body, fixes) = rewrite_body(&comment.text(), &map);
                new_bodies.push(new_body);
                per_body_fixes.push(fixes);
            }

            // Union of unique fixes across bodies, in first-seen order.
            let mut seen_words = HashSet::new();
            let mut ordered_fixes: Vec<(String, String)> = Vec::new();
            for fixes in &per_body_fixes {
                for (word, correction) in fixes {
                    if seen_words.insert(word.clone()) {
                        ordered_fixes.push((word.clone(), correction.clone()));
                    }
                }
            }

            if ordered_fixes.is_empty() {
                continue;
            }

            let tag_name = tag_token.text().to_string();
            let package = line.package_spec().and_then(|s| s.package_name());
            let info = line.info().filter(|s| !s.is_empty());

            let selector = OverrideLineSelector {
                tag: tag_name.clone(),
                info,
                package: package.clone(),
            };

            // One SetJustification carries every body rewrite for this
            // override; each unique (word, correction) becomes its own
            // diagnostic pointing at the same action set. Re-applying an
            // identical SetJustification is idempotent (already-corrected
            // text yields the same bytes), so recording every fixed tag
            // while the file is rewritten exactly once is safe.
            let action = Action::LintianOverrides(LintianOverridesAction::SetJustification {
                file: rel.clone(),
                selector,
                new_bodies,
            });

            for (word, correction) in &ordered_fixes {
                let mut issue = LintianIssue::source_with_info(
                    "capitalization-in-override-comment",
                    Visibility::Pedantic,
                    vec![tag_name.clone(), word.clone(), correction.clone()],
                );
                issue.package = package.clone();
                diagnostics.push(
                    Diagnostic::with_actions(
                        issue,
                        format!(
                            "Comment above override for {} contains a capitalization error: {} should be {}.",
                            tag_name, word, correction,
                        ),
                        "Fix capitalization errors in lintian override comments.",
                        vec![action.clone()],
                    )
                    .with_certainty(Certainty::Certain),
                );
            }
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "capitalization-in-override-comment",
    tags: ["capitalization-in-override-comment"],
    triggers: [
        debian_workspace::Trigger::File("debian/source/lintian-overrides"),
        debian_workspace::Trigger::Glob("debian/*.lintian-overrides"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = FsWorkspace::new(base.to_path_buf(), Some("test".into()), Some(version));
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn spell_map() -> HashMap<&'static str, &'static str> {
        SPELLING_CORRECTIONS_CASE.iter().copied().collect()
    }

    #[test]
    fn test_core_range() {
        let strip = |s: &str| {
            let r = core_range(s, 0..s.len());
            s[r].to_string()
        };
        assert_eq!(strip("linux"), "linux");
        assert_eq!(strip("(linux)"), "linux");
        assert_eq!(strip("linux,"), "linux");
        assert_eq!(strip("(linux!?"), "linux");
    }

    #[test]
    fn test_word_tokens_simple() {
        assert_eq!(word_tokens("foo bar"), vec![0..3, 4..7]);
        assert_eq!(word_tokens("  a  b  "), vec![2..3, 5..6]);
        assert_eq!(word_tokens(""), Vec::<Range<usize>>::new());
    }

    #[test]
    fn test_rewrite_body_simple() {
        let map = spell_map();
        let (new_body, fixes) = rewrite_body("Runs on linux.", &map);
        assert_eq!(new_body, "Runs on Linux.");
        assert_eq!(fixes, vec![("linux".to_string(), "Linux".to_string())]);
    }

    #[test]
    fn test_rewrite_body_multiple_words() {
        let map = spell_map();
        let (new_body, fixes) = rewrite_body("Built for linux with gnome.", &map);
        assert_eq!(new_body, "Built for Linux with GNOME.");
        assert_eq!(
            fixes,
            vec![
                ("linux".to_string(), "Linux".to_string()),
                ("gnome".to_string(), "GNOME".to_string()),
            ]
        );
    }

    #[test]
    fn test_rewrite_body_dedup_repeats() {
        let map = spell_map();
        let (new_body, fixes) = rewrite_body("linux and more linux.", &map);
        assert_eq!(new_body, "Linux and more Linux.");
        assert_eq!(fixes, vec![("linux".to_string(), "Linux".to_string())]);
    }

    #[test]
    fn test_rewrite_body_meta_package() {
        let map = spell_map();
        let (new_body, fixes) = rewrite_body("A meta package for linux.", &map);
        assert_eq!(new_body, "A metapackage for Linux.");
        assert_eq!(
            fixes,
            vec![
                ("meta package".to_string(), "metapackage".to_string()),
                ("linux".to_string(), "Linux".to_string()),
            ]
        );
    }

    #[test]
    fn test_rewrite_body_clean() {
        let map = spell_map();
        let (new_body, fixes) = rewrite_body("Runs on Linux.", &map);
        assert_eq!(new_body, "Runs on Linux.");
        assert!(fixes.is_empty());
    }

    fn write_overrides(base: &Path, content: &str) -> std::path::PathBuf {
        let source_dir = base.join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let path = source_dir.join("lintian-overrides");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_fix_capitalization() {
        let tmp = TempDir::new().unwrap();
        let path = write_overrides(
            tmp.path(),
            "# The tag fires because this runs on linux.\nsome-tag\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Fix capitalization errors in lintian override comments.",
        );
        let tags = result.fixed_lintian_tags();
        assert_eq!(tags, vec!["capitalization-in-override-comment"]);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# The tag fires because this runs on Linux.\nsome-tag\n",
        );
    }

    #[test]
    fn test_fix_multiple_words_in_one_comment() {
        let tmp = TempDir::new().unwrap();
        let path = write_overrides(
            tmp.path(),
            "# Built for linux with gnome support.\nsome-tag\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        let tags = result.fixed_lintian_tags();
        assert_eq!(
            tags,
            vec![
                "capitalization-in-override-comment",
                "capitalization-in-override-comment",
            ],
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# Built for Linux with GNOME support.\nsome-tag\n",
        );
    }

    #[test]
    fn test_fix_multi_line_justification() {
        let tmp = TempDir::new().unwrap();
        let path = write_overrides(
            tmp.path(),
            "# First line\n# Runs on linux systems.\nsome-tag\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# First line\n# Runs on Linux systems.\nsome-tag\n",
        );
    }

    #[test]
    fn test_no_justification_no_fix() {
        // Comment attached to nothing (blank line breaks the chain).
        let tmp = TempDir::new().unwrap();
        let original = "# Runs on linux\n\nsome-tag\n";
        let path = write_overrides(tmp.path(), original);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_clean_comment_no_fix() {
        let tmp = TempDir::new().unwrap();
        let original = "# Runs on Linux.\nsome-tag\n";
        let path = write_overrides(tmp.path(), original);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_binary_override_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("mypkg.lintian-overrides");
        fs::write(
            &path,
            "# Overrides for linux subsystem.\nmypkg binary: some-tag\n",
        )
        .unwrap();
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# Overrides for Linux subsystem.\nmypkg binary: some-tag\n",
        );
    }

    #[test]
    fn test_override_for_tag_left_alone() {
        // If the maintainer overrides `capitalization-in-override-comment`
        // itself, the fixer's `should_fix` gate stops the change.
        let tmp = TempDir::new().unwrap();
        let original = "capitalization-in-override-comment\n# Runs on linux\nsome-other-tag\n";
        let path = write_overrides(tmp.path(), original);
        assert!(matches!(
            run_apply(tmp.path()),
            Err(FixerError::NoChangesAfterOverrides(_)),
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
}
