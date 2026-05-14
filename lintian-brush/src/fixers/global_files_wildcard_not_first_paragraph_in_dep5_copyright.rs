use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use deb822_lossless::Deb822;
use debian_copyright::{pattern_depth, pattern_sort_key};
use debian_workspace::Workspace;
use std::path::PathBuf;
use std::str::FromStr;

const MARKER_WILDCARD: char = 'W';
const MARKER_OUTOFORDER: char = 'O';

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let bytes = match ws.read_file(&copyright_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };
    let Ok(deb822) = Deb822::from_str(&content) else {
        return Ok(Vec::new());
    };

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

    // Out-of-order: a depth-N paragraph followed somewhere later by a
    // depth-<N paragraph indicates a violation.
    for i in 0..files_info.len() {
        for j in (i + 1)..files_info.len() {
            if files_info[j].1 < files_info[i].1 {
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

    // Compute the desired order by sort_key.
    let mut sortable: Vec<_> = files_info
        .iter()
        .map(|(pattern, depth, _)| (pattern_sort_key(pattern, *depth), pattern.as_str()))
        .collect();
    sortable.sort_by(|a, b| a.0.cmp(&b.0));
    let desired_order: Vec<String> = sortable.into_iter().map(|(_, p)| p.to_string()).collect();

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

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    // If the only kind of diagnostic that fired was the wildcard-not-first
    // one, use the more specific phrasing. Otherwise use the generic
    // "reorder by depth" description.
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
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
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
}
