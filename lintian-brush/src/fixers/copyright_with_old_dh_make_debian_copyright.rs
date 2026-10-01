use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction, TextRange};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use regex::Regex;
use std::path::PathBuf;

const DESCRIPTION: &str = "Old dh_make copyright boilerplate in debian/copyright.";

/// Match the dh_make boilerplate line `The Debian packaging is (C) YYYY...`
/// that lintian flags via `copyright-with-old-dh-make-debian-copyright`.
///
/// Lintian uses `/The\s+Debian\s+packaging\s+is\s+\(C\)\s+\d+/i` on the raw
/// file contents. We capture `(C)` so the fixer can swap it for
/// `Copyright (C)` without touching the surrounding words.
fn boilerplate_re() -> Regex {
    Regex::new(r"(?i)The\s+Debian\s+packaging\s+is\s+(\(C\))\s+\d+").unwrap()
}

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

    let re = boilerplate_re();
    // ReplaceText actions re-read the file for each application, so applying
    // them in forward order would shift offsets of later matches. Reverse
    // the vector so the right-most edit happens first and leaves earlier
    // offsets unchanged.
    let mut actions: Vec<Action> = re
        .captures_iter(content)
        .map(|m| {
            let group = m.get(1).expect("regex has one capture group");
            Action::Filesystem(FilesystemAction::ReplaceText {
                file: copyright_rel.clone(),
                range: TextRange {
                    start: group.start(),
                    end: group.end(),
                },
                replacement: "Copyright (C)".into(),
            })
        })
        .collect();
    actions.reverse();

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source(
        "copyright-with-old-dh-make-debian-copyright",
        Visibility::Pedantic,
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        DESCRIPTION,
        "Replace bare \"(C)\" in dh_make copyright boilerplate with \"Copyright (C)\".",
        actions,
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "copyright-with-old-dh-make-debian-copyright",
    tags: ["copyright-with-old-dh-make-debian-copyright"],
    triggers: [debian_workspace::Trigger::File("debian/copyright")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = FsWorkspace::new(base, Some("test".into()), Some(version));
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_copyright(base: &Path, content: &str) -> std::path::PathBuf {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_regex_matches_simple() {
        let re = boilerplate_re();
        let text = "The Debian packaging is (C) 2008, Someone\n";
        let m = re.captures(text).unwrap();
        assert_eq!(m.get(1).unwrap().as_str(), "(C)");
    }

    #[test]
    fn test_regex_case_insensitive() {
        let re = boilerplate_re();
        assert!(re.is_match("the debian packaging is (C) 2010"));
        assert!(re.is_match("THE DEBIAN PACKAGING IS (C) 2010"));
    }

    #[test]
    fn test_regex_requires_year() {
        let re = boilerplate_re();
        assert!(!re.is_match("The Debian packaging is (C) nobody"));
    }

    #[test]
    fn test_regex_tolerates_whitespace() {
        let re = boilerplate_re();
        assert!(re.is_match("The  Debian\tpackaging  is   (C)   2010"));
    }

    #[test]
    fn test_regex_does_not_match_already_fixed() {
        let re = boilerplate_re();
        assert!(!re.is_match("The Debian packaging is Copyright 2008, Someone"));
    }

    #[test]
    fn test_fix_simple() {
        let tmp = TempDir::new().unwrap();
        let path = write_copyright(
            tmp.path(),
            "This package was debianized by Russ Allbery <rra@debian.org> on\nMon, 29 Dec 2008 17:33:59 -0800.\n\nThe Debian packaging is (C) 2008, Russ Allbery <rra@debian.org> and\nis licensed under the GPL, see `/usr/share/common-licenses/GPL'.\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Replace bare \"(C)\" in dh_make copyright boilerplate with \"Copyright (C)\"."
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("copyright-with-old-dh-make-debian-copyright")
        );

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "This package was debianized by Russ Allbery <rra@debian.org> on\nMon, 29 Dec 2008 17:33:59 -0800.\n\nThe Debian packaging is Copyright (C) 2008, Russ Allbery <rra@debian.org> and\nis licensed under the GPL, see `/usr/share/common-licenses/GPL'.\n",
        );
    }

    #[test]
    fn test_no_match_leaves_file_alone() {
        let tmp = TempDir::new().unwrap();
        write_copyright(
            tmp.path(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: foo\n\nFiles: *\nCopyright: 2026 Someone\nLicense: GPL-3+\n",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_fixed_is_noop() {
        let tmp = TempDir::new().unwrap();
        write_copyright(
            tmp.path(),
            "The Debian packaging is Copyright (C) 2008, Russ Allbery <rra@debian.org>.\n",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_fix_multiple_occurrences() {
        let tmp = TempDir::new().unwrap();
        let path = write_copyright(
            tmp.path(),
            "The Debian packaging is (C) 2008, Alice <alice@example.com>.\n\nThe Debian packaging is (C) 2010, Bob <bob@example.com>.\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "The Debian packaging is Copyright (C) 2008, Alice <alice@example.com>.\n\nThe Debian packaging is Copyright (C) 2010, Bob <bob@example.com>.\n",
        );
    }

    #[test]
    fn test_fix_preserves_surrounding_whitespace() {
        let tmp = TempDir::new().unwrap();
        let path = write_copyright(
            tmp.path(),
            "The  Debian\tpackaging  is  (C)  2010, Someone.\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "The  Debian\tpackaging  is  Copyright (C)  2010, Someone.\n",
        );
    }
}
