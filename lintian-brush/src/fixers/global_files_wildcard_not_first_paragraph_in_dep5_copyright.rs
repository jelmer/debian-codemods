use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_copyright::{pattern_depth, GlobPattern};
use debian_workspace::Workspace;
use std::path::PathBuf;

const MARKER_WILDCARD: char = 'W';
const MARKER_OUTOFORDER: char = 'O';

/// A representative path that the given Files pattern matches.
///
/// Trailing `/*` or `/` is stripped so that a shallower pattern's glob, which
/// typically ends in `/*`, can be tested against it. Returns `None` when the
/// pattern contains a backslash escape, which `GlobPattern` would need to
/// interpret and could reject; in that case we conservatively decline to
/// reason about overlap.
fn representative_path(pattern: &str) -> Option<&str> {
    if pattern.contains('\\') {
        return None;
    }
    let trimmed = pattern
        .strip_suffix("/*")
        .or_else(|| pattern.strip_suffix('/'))
        .unwrap_or(pattern);
    Some(trimmed)
}

/// Whether `shallower` would match files covered by `deeper`.
///
/// This mirrors lintian's own check, which only flags two patterns as out of
/// order when the later, less specific pattern actually overrides files
/// matched by the earlier, more specific one. Patterns in disjoint subtrees
/// never overlap and must keep their relative order.
fn overlaps(deeper: &str, shallower: &str) -> bool {
    if shallower.contains('\\') {
        return false;
    }
    let Some(repr) = representative_path(deeper) else {
        return false;
    };
    GlobPattern::new(shallower).is_match(repr)
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
    let deb822 = copyright.as_deb822();

    // Snapshot all Files paragraphs with their pattern, depth, and line.
    let mut files_info: Vec<(String, usize, usize)> = Vec::new();
    for para in deb822.paragraphs() {
        let Some(files_value) = para.get("Files") else {
            continue;
        };
        let pattern = files_value
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        let depth = pattern_depth(&pattern);
        let line = para.line() + 1;
        files_info.push((pattern, depth, line));
    }
    if files_info.is_empty() {
        return Ok(Vec::new());
    }

    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Wildcard-not-first: only when the wildcard exists and isn't already
    // at the head of the Files-paragraph sequence.
    let wildcard_pos = files_info.iter().position(|(p, _, _)| p == "*");
    if let Some(pos) = wildcard_pos {
        if pos > 0 {
            let line = files_info[pos].2;
            let issue = LintianIssue::source_with_info(
                "global-files-wildcard-not-first-paragraph-in-dep5-copyright",
                Visibility::Warning,
                vec![format!("[debian/copyright:{}]", line)],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                "Global Files wildcard is not the first paragraph.",
                format!("{}", MARKER_WILDCARD),
                Vec::new(),
            ));
        }
    }

    // Out-of-order: a deeper paragraph followed later by a shallower
    // paragraph is only a violation when the shallower pattern actually
    // overlaps the deeper one, i.e. would override files it matches.
    // Disjoint subtrees may appear in any relative order.
    for i in 0..files_info.len() {
        for j in (i + 1)..files_info.len() {
            if files_info[j].1 < files_info[i].1 && overlaps(&files_info[i].0, &files_info[j].0) {
                let issue = LintianIssue::source_with_info(
                    "globbing-patterns-out-of-order",
                    Visibility::Warning,
                    vec![format!(
                        "{} {} [debian/copyright:{}]",
                        files_info[i].0, files_info[j].0, files_info[j].2
                    )],
                );
                diagnostics.push(Diagnostic::with_actions(
                    issue,
                    "Files paragraphs are out of glob order.",
                    format!("{}", MARKER_OUTOFORDER),
                    Vec::new(),
                ));
                break;
            }
        }
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    let desired_order = reorder(&files_info);

    // The single ReorderParagraphs action is shared across all
    // diagnostics — they each describe a contributing issue but the
    // applied edit is one structural rearrangement.
    let action = Action::Deb822(Deb822Action::ReorderParagraphs {
        file: copyright_rel,
        key_field: "Files".into(),
        order: desired_order,
    });
    if let Some(first) = diagnostics.first_mut() {
        first.plans[0].actions.push(action);
    }

    Ok(diagnostics)
}

/// Whether `general` is strictly more general than `specific`: it matches the
/// files `specific` covers, but not the other way around. A strictly more
/// general pattern must come first, since in DEP-5 the last matching paragraph
/// wins. `*` is strictly more general than any other pattern.
fn strictly_more_general(general: &str, specific: &str) -> bool {
    overlaps(specific, general) && !overlaps(general, specific)
}

/// Compute a minimal paragraph order that resolves overlap-based violations.
///
/// In DEP-5 the last matching paragraph wins, so a general pattern must come
/// before the specific ones that refine it. Paragraphs are placed in their
/// original order, but each is moved ahead of any earlier-placed paragraph it
/// must precede: one it is strictly more general than. Because `*` overlaps
/// everything and is more general than any other pattern, this brings a global
/// wildcard to the front. Disjoint paragraphs impose no constraint and keep
/// their relative order.
fn reorder(files_info: &[(String, usize, usize)]) -> Vec<String> {
    let mut order: Vec<&str> = Vec::with_capacity(files_info.len());
    for (pattern, _, _) in files_info {
        // Find the earliest placed paragraph this one must precede: one it is
        // strictly more general than, which must come after it.
        let insert_at = order
            .iter()
            .position(|placed| strictly_more_general(pattern, placed));
        match insert_at {
            Some(pos) => order.insert(pos, pattern),
            None => order.push(pattern),
        }
    }
    order.into_iter().map(|p| p.to_string()).collect()
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    // If the only kind of diagnostic that fired was the wildcard-not-first
    // one, use the more specific phrasing. Otherwise use the generic
    // reorder description.
    let only_wildcard = fixed.iter().all(|(d, _)| {
        d.plans
            .first()
            .map(|p| p.label.starts_with(MARKER_WILDCARD))
            .unwrap_or(false)
    });
    if only_wildcard {
        "Make \"Files: *\" paragraph the first in the copyright file.".to_string()
    } else {
        "Reorder Files paragraphs in debian/copyright by directory depth.".to_string()
    }
}

declare_detector! {
    name: "global-files-wildcard-not-first-paragraph-in-dep5-copyright",
    tags: ["global-files-wildcard-not-first-paragraph-in-dep5-copyright", "globbing-patterns-out-of-order"],
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
    use deb822_lossless::Deb822;
    use std::fs;
    use std::path::Path;
    use std::str::FromStr;
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
    fn test_wildcard_not_first() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let copyright = debian.join("copyright");
        fs::write(
            &copyright,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: src/*\nCopyright: 2020 Author\nLicense: MIT\n\nFiles: *\nCopyright: 2020 Author\nLicense: MIT\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        let updated = fs::read_to_string(&copyright).unwrap();
        let deb = Deb822::from_str(&updated).unwrap();
        let paragraphs: Vec<_> = deb.paragraphs().collect();
        assert_eq!(paragraphs[1].get("Files").unwrap().trim(), "*");
        assert_eq!(paragraphs[2].get("Files").unwrap().trim(), "src/*");
    }

    #[test]
    fn test_out_of_order_patterns() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let copyright = debian.join("copyright");
        fs::write(
            &copyright,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: src/foo/bar/*\nCopyright: 2020 Author\nLicense: MIT\n\nFiles: src/*\nCopyright: 2020 Another\nLicense: GPL-2\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        let updated = fs::read_to_string(&copyright).unwrap();
        let deb = Deb822::from_str(&updated).unwrap();
        let paragraphs: Vec<_> = deb.paragraphs().collect();
        assert_eq!(paragraphs[1].get("Files").unwrap().trim(), "src/*");
        assert_eq!(paragraphs[2].get("Files").unwrap().trim(), "src/foo/bar/*");
    }

    #[test]
    fn test_already_sorted() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: 2020 Generic\nLicense: GPL-2\n\nFiles: src/*\nCopyright: 2020 Another\nLicense: Apache-2.0\n\nFiles: src/foo/*\nCopyright: 2020 Author\nLicense: MIT\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    /// A deeper pattern preceding a shallower one in a disjoint subtree is not
    /// out of order: the shallower pattern does not match the deeper one's
    /// files. Regression test for Debian bug #1135874 (numpy).
    #[test]
    fn test_disjoint_subtrees_left_alone() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: numpy/random/src/distributions/*\nCopyright: 2020 Author\nLicense: MIT\n\nFiles: numpy/linalg/lapack_lite/*\nCopyright: 2020 Another\nLicense: BSD-3-clause\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    /// A specific file deep in one subtree appearing before a shallow wildcard
    /// for a different subtree must not trigger a reorder.
    #[test]
    fn test_specific_file_before_disjoint_wildcard() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: numpy/core/src/common/npy_cpuinfo_parser.h\nCopyright: 2020 Author\nLicense: MIT\n\nFiles: numpy/fft/*\nCopyright: 2020 Another\nLicense: BSD-3-clause\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    /// Overlapping subtrees are still reordered: a deeper pattern overridden by
    /// a later shallower pattern that matches its files is a genuine violation.
    #[test]
    fn test_overlapping_subtree_reordered() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let copyright = debian.join("copyright");
        fs::write(
            &copyright,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: numpy/random/src/distributions/*\nCopyright: 2020 Author\nLicense: MIT\n\nFiles: numpy/random/*\nCopyright: 2020 Another\nLicense: BSD-3-clause\n",
        )
        .unwrap();
        run_apply(tmp.path()).unwrap();
        let updated = fs::read_to_string(&copyright).unwrap();
        let deb = Deb822::from_str(&updated).unwrap();
        let paragraphs: Vec<_> = deb.paragraphs().collect();
        assert_eq!(paragraphs[1].get("Files").unwrap().trim(), "numpy/random/*");
        assert_eq!(
            paragraphs[2].get("Files").unwrap().trim(),
            "numpy/random/src/distributions/*"
        );
    }

    /// Reordering preserves disjoint paragraphs in place while fixing only the
    /// overlapping violation.
    #[test]
    fn test_reorder_is_minimal() {
        let order = reorder(&[
            ("numpy/random/src/distributions/*".into(), 4, 1),
            ("numpy/linalg/lapack_lite/*".into(), 3, 2),
            ("numpy/random/*".into(), 2, 3),
        ]);
        assert_eq!(
            order,
            vec![
                "numpy/random/*".to_string(),
                "numpy/random/src/distributions/*".to_string(),
                "numpy/linalg/lapack_lite/*".to_string(),
            ]
        );
    }
}
