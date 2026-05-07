use crate::declare_detector;
use crate::diagnostic::{Action, ChangelogAction, Diagnostic};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_changelog::iter_changes_by_author;
use std::path::PathBuf;

const TEAM_UPLOAD_LINE: &str = "  * Team upload.";

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let changelog_rel = PathBuf::from("debian/changelog");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };
    let uploaders_str = control
        .source()
        .and_then(|s| s.as_deb822().get("Uploaders").map(|v| v.to_string()))
        .unwrap_or_default();
    let uploader_emails: Vec<String> = uploaders_str
        .split(',')
        .map(|entry| {
            let (_, email) = debian_changelog::parseaddr(entry.trim());
            email.to_string()
        })
        .collect();

    let changelog = match ws.parsed_changelog() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let Some(last_entry) = changelog.iter().next() else {
        return Ok(Vec::new());
    };
    if last_entry.is_unreleased() != Some(true) {
        return Ok(Vec::new());
    }
    let author_email = last_entry.email().unwrap_or_default();
    if !uploader_emails.contains(&author_email) {
        return Ok(Vec::new());
    }

    let last_package = last_entry.package();
    let last_version = last_entry.version();

    let mut occurrence_counts: std::collections::HashMap<(Option<String>, String), usize> =
        std::collections::HashMap::new();
    for change in iter_changes_by_author(&changelog) {
        if change.package() != last_package || change.version() != last_version {
            continue;
        }
        for bullet in change.split_into_bullets() {
            let lines = bullet.lines();
            let text = lines.join("\n");
            let author = bullet.author().map(|s| s.to_string());
            let key = (author.clone(), text.clone());
            let occurrence = *occurrence_counts.entry(key.clone()).or_insert(0);
            occurrence_counts.insert(key, occurrence + 1);

            let is_team_upload = lines
                .iter()
                .any(|line| line.trim() == TEAM_UPLOAD_LINE.trim());
            if !is_team_upload {
                continue;
            }

            let line_num = bullet
                .line_numbers()
                .first()
                .copied()
                .map(|n| n + 1)
                .unwrap_or(1);
            let issue = LintianIssue::source_with_info(
                "unnecessary-team-upload",
                vec![format!("[debian/changelog:{}]", line_num)],
            );
            let Some(version) = last_version.as_ref() else {
                return Ok(Vec::new());
            };
            return Ok(vec![Diagnostic::with_actions(
                issue,
                "Remove unnecessary Team Upload line in changelog.",
                vec![Action::Changelog(ChangelogAction::RemoveBullet {
                    file: changelog_rel.clone(),
                    version: version.to_string(),
                    author,
                    text,
                    occurrence,
                })],
            )]);
        }
    }

    Ok(Vec::new())
}

declare_detector! {
    name: "unnecessary-team-upload",
    tags: ["unnecessary-team-upload"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Uploaders",
        },
        crate::workspace::Trigger::Changelog(crate::workspace::ChangelogAspect::Body),
        crate::workspace::Trigger::Changelog(crate::workspace::ChangelogAspect::Maintainer),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test-pkg", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_unnecessary_team_upload() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-pkg\nMaintainer: Team <team@example.com>\nUploaders: John Doe <john@example.com>\n",
        )
        .unwrap();
        let changelog = debian.join("changelog");
        fs::write(
            &changelog,
            "test-pkg (1.0-2) UNRELEASED; urgency=medium\n\n  * Team upload.\n\n  [ John Doe ]\n  * Some change\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&changelog).unwrap(),
            "test-pkg (1.0-2) UNRELEASED; urgency=medium\n\n  [ John Doe ]\n  * Some change\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        );
    }

    #[test]
    fn test_no_change_when_not_unreleased() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-pkg\nMaintainer: Team <team@example.com>\nUploaders: John Doe <john@example.com>\n",
        )
        .unwrap();
        fs::write(
            debian.join("changelog"),
            "test-pkg (1.0-2) unstable; urgency=medium\n\n  * Team upload.\n\n  [ John Doe ]\n  * Some change\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_author_not_uploader() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-pkg\nMaintainer: Team <team@example.com>\nUploaders: Someone Else <other@example.com>\n",
        )
        .unwrap();
        fs::write(
            debian.join("changelog"),
            "test-pkg (1.0-2) UNRELEASED; urgency=medium\n\n  * Team upload.\n\n  [ John Doe ]\n  * Some change\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
