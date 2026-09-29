use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// Positions (1-indexed) of extra compat-level lines beyond position 1.
///
/// Mirrors the check in lintian's `Debhelper.pm`: line 1 is treated as the
/// compat level, and any subsequent line starting with a digit is a duplicate.
fn extra_level_positions(content: &str) -> Vec<usize> {
    content
        .split_inclusive('\n')
        .enumerate()
        .skip(1)
        .filter_map(|(idx, line)| {
            if line.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                Some(idx + 1)
            } else {
                None
            }
        })
        .collect()
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let compat_rel = PathBuf::from("debian/compat");
    let bytes = match ws.read_file(&compat_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };

    let positions = extra_level_positions(content);
    if positions.is_empty() {
        return Ok(Vec::new());
    }

    let Some(first_line) = content.lines().next() else {
        return Ok(Vec::new());
    };
    let level = first_line.trim();
    if level.is_empty() || !level.chars().all(|c| c.is_ascii_digit()) {
        // First line isn't a bare compat level; don't guess.
        return Ok(Vec::new());
    }

    let new_content = format!("{}\n", level);

    let action = Action::Filesystem(FilesystemAction::Write {
        file: compat_rel,
        content: new_content.into_bytes(),
    });

    let description = "debian/compat contains multiple compatibility levels.".to_string();
    let label = "Reduce debian/compat to a single compatibility level.".to_string();

    let mut diagnostics = Vec::new();
    for (i, position) in positions.into_iter().enumerate() {
        let issue = LintianIssue::source_with_info(
            "debhelper-compat-file-contains-multiple-levels",
            Visibility::Error,
            vec![format!("[debian/compat:{}]", position)],
        );
        let actions = if i == 0 {
            vec![action.clone()]
        } else {
            Vec::new()
        };
        diagnostics.push(
            Diagnostic::with_actions(issue, description.clone(), label.clone(), actions)
                .with_certainty(Certainty::Certain),
        );
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "debhelper-compat-file-contains-multiple-levels",
    tags: ["debhelper-compat-file-contains-multiple-levels"],
    triggers: [debian_workspace::Trigger::File("debian/compat")],
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
    fn test_extra_level_positions_none() {
        assert_eq!(extra_level_positions("13\n"), Vec::<usize>::new());
    }

    #[test]
    fn test_extra_level_positions_trailing_digit_line() {
        assert_eq!(extra_level_positions("13\n12\n"), vec![2]);
    }

    #[test]
    fn test_extra_level_positions_multiple_extras() {
        assert_eq!(extra_level_positions("13\n12\n11\n"), vec![2, 3]);
    }

    #[test]
    fn test_extra_level_positions_ignores_non_digit_lines() {
        assert_eq!(
            extra_level_positions("13\n# comment\n"),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn test_fix_simple_duplicate() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let compat = debian.join("compat");
        fs::write(&compat, "13\n12\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Reduce debian/compat to a single compatibility level."
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag,
            Some("debhelper-compat-file-contains-multiple-levels".to_string())
        );
        assert_eq!(
            result.fixed_lintian_issues[0].info,
            Some("[debian/compat:2]".to_string())
        );
        assert_eq!(fs::read_to_string(&compat).unwrap(), "13\n");
    }

    #[test]
    fn test_fix_many_extra_lines() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let compat = debian.join("compat");
        fs::write(&compat, "13\n12\n11\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 2);
        assert_eq!(
            result.fixed_lintian_issues[0].info,
            Some("[debian/compat:2]".to_string())
        );
        assert_eq!(
            result.fixed_lintian_issues[1].info,
            Some("[debian/compat:3]".to_string())
        );
        assert_eq!(fs::read_to_string(&compat).unwrap(), "13\n");
    }

    #[test]
    fn test_no_extra_levels() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("compat"), "13\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_first_line_not_digit_backs_off() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        // First line is not a bare compat level, so we can't tell what the
        // real level is; the fixer should not touch the file.
        fs::write(debian.join("compat"), "not-a-number\n12\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_empty_first_line_backs_off() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("compat"), "\n12\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_compat_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
