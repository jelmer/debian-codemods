use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction, TextRange};
use debian_workspace::Workspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use chrono::Datelike;
use rowan::ast::AstNode;
use std::path::PathBuf;

const CHANGELOG_REL: &str = "debian/changelog";

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let changelog = match ws.parsed_changelog() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut diagnostics = Vec::new();

    for entry in changelog.iter() {
        let ts_node = match entry.timestamp_node() {
            Some(t) => t,
            None => continue,
        };
        let ts_range = ts_node.syntax().text_range();
        let ts_text = ts_node.syntax().text().to_string();

        // The timestamp's text begins with the day-of-week followed by
        // ", ". Anything else is unparseable for our purposes — leave it.
        let comma_off = match ts_text.find(", ") {
            Some(off) => off,
            None => continue,
        };
        let orig_dow = &ts_text[..comma_off];
        let date_time_part = &ts_text[comma_off + 2..];

        let parsed_date =
            match chrono::DateTime::parse_from_str(date_time_part, "%d %b %Y %H:%M:%S %z") {
                Ok(dt) => dt,
                Err(_) => continue,
            };

        let new_full = parsed_date.to_rfc2822();
        let new_dow = match new_full.split(',').next() {
            Some(s) => s,
            None => continue,
        };

        if new_dow == orig_dow {
            continue;
        }

        // Build a Filesystem::ReplaceText action over just the
        // day-of-week portion (the bytes before `,`). The byte offsets
        // come straight from the rowan node so they're precise even when
        // the file mixes tabs and other whitespace.
        let ts_start: usize = ts_range.start().into();
        let dow_range = TextRange {
            start: ts_start,
            end: ts_start + comma_off,
        };

        let issue = LintianIssue::source_with_info(
            "debian-changelog-has-wrong-day-of-week",
            Visibility::Warning,
            vec![format!(
                "{:04}-{:02}-{:02} is a {}",
                parsed_date.year(),
                parsed_date.month(),
                parsed_date.day(),
                parsed_date.format("%A")
            )],
        );

        let label = match entry.version() {
            Some(v) => format!("Fix day-of-week for changelog entry {}.", v),
            None => "Fix day-of-week for changelog entry.".to_string(),
        };
        let description = match entry.version() {
            Some(v) => format!("Wrong day-of-week in changelog entry {}.", v),
            None => "Wrong day-of-week in changelog entry.".to_string(),
        };

        diagnostics.push(Diagnostic::with_actions(
            issue,
            description,
            label,
            vec![Action::Filesystem(FilesystemAction::ReplaceText {
                file: PathBuf::from(CHANGELOG_REL),
                range: dow_range,
                replacement: new_dow.to_string(),
            })],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-changelog-has-wrong-day-of-week",
    tags: ["debian-changelog-has-wrong-day-of-week"],
    triggers: [debian_workspace::Trigger::Changelog(
        debian_workspace::ChangelogAspect::Timestamp,
    )],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use debian_workspace::{DetectorAdapter, TreeWorkspace};
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = TreeWorkspace::new(base, "test", "1.0".parse().unwrap());
        detect(&ws, &FixerPreferences::default())
    }

    #[test]
    fn detect_emits_replace_action_for_wrong_day_of_week() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // April 22, 2018 was a Sunday, not Monday.
        let changelog_content = "foo (1.0) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 22 Apr 2018 00:58:14 +0000\n";
        fs::write(debian_dir.join("changelog"), changelog_content).unwrap();

        let diags = detect_in(temp_dir.path()).unwrap();
        assert_eq!(diags.len(), 1);
        // The action should target only the day-of-week portion.
        let Action::Filesystem(FilesystemAction::ReplaceText {
            ref file,
            ref range,
            ref replacement,
        }) = diags[0].plans[0].actions[0]
        else {
            panic!("expected a ReplaceText action");
        };
        assert_eq!(file, &PathBuf::from("debian/changelog"));
        assert_eq!(replacement, "Sun");
        assert_eq!(&changelog_content[range.start..range.end], "Mon");
    }

    #[test]
    fn apply_fixes_wrong_day_of_week() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let changelog_content = "foo (1.0) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 22 Apr 2018 00:58:14 +0000\n";
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert!(result.description.contains("day-of-week"));

        let updated = fs::read_to_string(&changelog_path).unwrap();
        assert!(updated.contains("Sun, 22 Apr 2018"));
        assert!(!updated.contains("Mon, 22 Apr 2018"));
    }

    #[test]
    fn no_change_when_day_correct() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // April 22, 2018 was a Sunday — already correct.
        let changelog_content = "foo (1.0) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Sun, 22 Apr 2018 00:58:14 +0000\n";
        fs::write(debian_dir.join("changelog"), changelog_content).unwrap();

        assert!(detect_in(temp_dir.path()).unwrap().is_empty());
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
