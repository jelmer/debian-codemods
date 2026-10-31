use crate::declare_detector;
use crate::diagnostic::{
    Action, ActionPlan, Deb822Action, Diagnostic, IndentPattern, ParagraphSelector,
};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Drop runs of a repeated pattern from a Files value, keeping the
/// first occurrence and preserving the surrounding line layout.
///
/// Returns `None` if removal would leave the field empty, which would
/// be invalid.
fn dedupe_pattern(value: &str, pattern: &str) -> Option<String> {
    let mut seen = false;
    let mut lines = Vec::new();
    for line in value.lines() {
        let mut kept = Vec::new();
        for token in line.split_whitespace() {
            if token == pattern {
                if seen {
                    continue;
                }
                seen = true;
            }
            kept.push(token);
        }
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

    // For each wildcard, collect (paragraph index, line, count-in-paragraph).
    // Lintian uses `trimmed_list` on the Files field, which splits on
    // whitespace; commas inside a Files token stay part of the token.
    let mut by_pattern: BTreeMap<String, Vec<(usize, usize, usize)>> = BTreeMap::new();

    for (idx, fp) in copyright.iter_files().enumerate() {
        let paragraph = fp.as_deb822();
        let line_no = paragraph
            .entries()
            .find(|e| e.key().as_deref() == Some("Files"))
            .map(|e| e.line() + 1)
            .unwrap_or_else(|| paragraph.line() + 1);
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for pattern in fp.files() {
            *counts.entry(pattern).or_default() += 1;
        }
        for (pattern, count) in counts {
            by_pattern
                .entry(pattern)
                .or_default()
                .push((idx, line_no, count));
        }
    }

    let mut diagnostics = Vec::new();

    for (pattern, occurrences) in by_pattern {
        // Total number of times this exact wildcard appears across all
        // Files fields. Lintian pushes one entry per occurrence, so a
        // single-paragraph duplicate counts as 2 and triggers the tag
        // just like the cross-paragraph case.
        let total: usize = occurrences.iter().map(|(_, _, c)| c).sum();
        if total < 2 {
            continue;
        }

        // Reconstruct the "(lines L1 L2 ...)" string in lintian's
        // order: one line entry per occurrence, with intra-paragraph
        // duplicates emitting the same line repeatedly.
        let lines_str = occurrences
            .iter()
            .flat_map(|(_, line, count)| std::iter::repeat_n(line.to_string(), *count))
            .collect::<Vec<_>>()
            .join(" ");

        let first_line = occurrences[0].1;
        let issue = LintianIssue::source_with_info(
            "duplicate-globbing-patterns",
            Visibility::Error,
            vec![
                pattern.clone(),
                format!("(lines {})", lines_str),
                format!("[debian/copyright:{}]", first_line),
            ],
        );

        // Only the single-paragraph case is safe to fix automatically.
        // When the duplication spans paragraphs, each paragraph carries
        // its own copyright and license information; collapsing them
        // would discard those details.
        let can_fix = occurrences.len() == 1 && occurrences[0].2 > 1;
        let actions: Vec<Action> = if can_fix {
            let (idx, _, _) = occurrences[0];
            let paragraph = copyright
                .iter_files()
                .nth(idx)
                .expect("iter_files is stable within one tree");
            let files_value = paragraph.as_deb822().get("Files").unwrap_or_default();
            match dedupe_pattern(&files_value, &pattern) {
                Some(new_value) => vec![Action::Deb822(Deb822Action::SetFieldWithIndent {
                    file: copyright_rel.clone(),
                    paragraph: ParagraphSelector::CopyrightFiles { glob: files_value },
                    field: "Files".into(),
                    value: new_value,
                    indent: IndentPattern::Fixed { spaces: 1 },
                })],
                None => Vec::new(),
            }
        } else {
            Vec::new()
        };

        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("pattern\t{}", pattern),
            format!(
                "debian/copyright: Drop duplicate globbing pattern {}.",
                pattern
            ),
            actions,
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let patterns: Vec<&str> = fixed
        .iter()
        .filter_map(|(d, _)| d.message.strip_prefix("pattern\t"))
        .collect();
    format!(
        "debian/copyright: Drop duplicate globbing pattern{} {}.",
        if patterns.len() == 1 { "" } else { "s" },
        patterns.join(", ")
    )
}

declare_detector! {
    name: "duplicate-globbing-patterns",
    tags: ["duplicate-globbing-patterns"],
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

    const HEADER: &str =
        "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n";

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
    fn test_dedupe_pattern_single_line() {
        assert_eq!(
            dedupe_pattern("a.c b.c a.c", "a.c").unwrap(),
            "a.c b.c".to_string()
        );
    }

    #[test]
    fn test_dedupe_pattern_multi_line() {
        assert_eq!(
            dedupe_pattern("a.c b.c\n a.c c.c", "a.c").unwrap(),
            "a.c b.c\nc.c".to_string()
        );
    }

    #[test]
    fn test_dedupe_pattern_keeps_other_duplicates() {
        // Only the named pattern is deduplicated; `b.c` repeating is
        // reported separately and left alone here.
        assert_eq!(
            dedupe_pattern("a.c b.c a.c b.c", "a.c").unwrap(),
            "a.c b.c b.c".to_string()
        );
    }

    #[test]
    fn test_dedupe_pattern_collapses_to_single_token() {
        assert_eq!(dedupe_pattern("a.c a.c", "a.c"), Some("a.c".into()));
    }

    #[test]
    fn test_dedupe_pattern_declines_to_empty_field() {
        // There is no way to drop the duplication when the entire
        // Files value is a single token; the field would be left
        // empty, which is invalid.
        assert_eq!(dedupe_pattern("", "a.c"), None);
    }

    #[test]
    fn test_single_paragraph_duplicate_is_fixed() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            format!(
                "{}Files: src/* man/* src/*\nCopyright: 2020 Someone\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
                HEADER
            ),
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "debian/copyright: Drop duplicate globbing pattern src/*."
        );
        assert_eq!(
            fs::read_to_string(debian.join("copyright")).unwrap(),
            format!(
                "{}Files: src/* man/*\nCopyright: 2020 Someone\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
                HEADER
            )
        );
    }

    #[test]
    fn test_cross_paragraph_duplicate_is_reported_not_fixed() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let copyright = format!(
            "{}Files: src/*\nCopyright: 2020 Someone\nLicense: GPL-2\n\nFiles: src/*\nCopyright: 2020 Other\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
            HEADER
        );
        fs::write(debian.join("copyright"), &copyright).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(debian.join("copyright")).unwrap(),
            copyright
        );
    }

    #[test]
    fn test_no_duplicates_no_changes() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let copyright = format!(
            "{}Files: src/* man/*\nCopyright: 2020 Someone\nLicense: GPL-2\n\nFiles: doc/*\nCopyright: 2020 Other\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
            HEADER
        );
        fs::write(debian.join("copyright"), &copyright).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(debian.join("copyright")).unwrap(),
            copyright
        );
    }

    #[test]
    fn test_not_machine_readable() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "This is not a machine-readable copyright file.\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_duplicate_on_separate_lines_in_same_paragraph() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            format!(
                "{}Files: src/*\n man/*\n src/*\nCopyright: 2020 Someone\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
                HEADER
            ),
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "debian/copyright: Drop duplicate globbing pattern src/*."
        );
        let after = fs::read_to_string(debian.join("copyright")).unwrap();
        // The duplicate occurrence (on its own line) is dropped, and
        // the remaining patterns keep their line layout.
        assert_eq!(
            after,
            format!(
                "{}Files: src/*\n man/*\nCopyright: 2020 Someone\nLicense: GPL-2\n\nLicense: GPL-2\n Text.\n",
                HEADER
            )
        );
    }
}
