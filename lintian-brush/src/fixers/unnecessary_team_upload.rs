use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_changelog::ChangeLog;
use std::fs;
use std::path::Path;

const TEAM_UPLOAD_LINE: &str = "  * Team upload.";

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let changelog_path = base_path.join("debian/changelog");
    let control_path = base_path.join("debian/control");

    if !changelog_path.exists() || !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Parse control file to get uploader emails
    let editor = TemplatedControlEditor::open(&control_path)?;
    let uploaders_str = editor
        .source()
        .and_then(|s| s.as_deb822().get("Uploaders").map(|v| v.to_string()))
        .unwrap_or_default();

    // Extract email addresses from Uploaders field
    let mut uploader_emails = Vec::new();
    for entry in uploaders_str.split(',') {
        let (_, email) = debian_changelog::parseaddr(entry.trim());
        uploader_emails.push(email.to_string());
    }

    // Parse changelog
    let content = fs::read_to_string(&changelog_path)?;
    let changelog: ChangeLog = content
        .parse()
        .map_err(|e| FixerError::Other(format!("Failed to parse changelog: {}", e)))?;

    // Get the first (most recent) entry
    let last_entry = match changelog.iter().next() {
        Some(entry) => entry,
        None => return Err(FixerError::NoChanges),
    };

    // Check if distribution is UNRELEASED
    if last_entry.is_unreleased() != Some(true) {
        return Err(FixerError::NoChanges);
    }

    // Get author email
    let author_email = last_entry.email().unwrap_or_default();

    // If author is not in uploaders list, exit
    if !uploader_emails.contains(&author_email) {
        return Err(FixerError::NoChanges);
    }

    // Use iter_changes_by_author to get Change objects with split_into_bullets() support
    let changes = debian_changelog::iter_changes_by_author(&changelog);

    // Find and remove the "Team upload" bullet
    let mut found_team_upload = false;
    let mut team_upload_line_num = None;
    for change in changes {
        // Only process changes from the first (most recent) entry
        if change.package() != last_entry.package() || change.version() != last_entry.version() {
            continue;
        }

        let bullets = change.split_into_bullets();
        for bullet in bullets {
            let lines = bullet.lines();
            for line in lines.iter() {
                if line.trim() == TEAM_UPLOAD_LINE.trim() {
                    found_team_upload = true;
                    // Get the first line number for this bullet (1-indexed)
                    team_upload_line_num = Some(
                        bullet
                            .line_numbers()
                            .first()
                            .expect("bullet should have line numbers")
                            + 1,
                    );
                    bullet.remove();
                    break;
                }
            }
            if found_team_upload {
                break;
            }
        }
        if found_team_upload {
            break;
        }
    }

    if !found_team_upload {
        return Err(FixerError::NoChanges);
    }

    let line_num = team_upload_line_num.unwrap_or(1);
    let issue = LintianIssue::source_with_info(
        "unnecessary-team-upload",
        vec![format!("[debian/changelog:{}]", line_num)],
    );

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Write back the modified changelog
    fs::write(&changelog_path, changelog.to_string())?;

    Ok(
        FixerResult::builder("Remove unnecessary Team Upload line in changelog.")
            .fixed_issues(vec![issue])
            .build(),
    )
}

declare_fixer! {
    name: "unnecessary-team-upload",
    tags: ["unnecessary-team-upload"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_remove_unnecessary_team_upload() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Maintainer: Team <team@example.com>
Uploaders: John Doe <john@example.com>
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let changelog_content = r#"test-pkg (1.0-2) UNRELEASED; urgency=medium

  * Team upload.

  [ John Doe ]
  * Some change

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test-pkg", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&changelog_path).unwrap();
        assert!(!updated_content.contains("Team upload"));
    }

    #[test]
    fn test_no_change_when_not_unreleased() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Maintainer: Team <team@example.com>
Uploaders: John Doe <john@example.com>
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let changelog_content = r#"test-pkg (1.0-2) unstable; urgency=medium

  * Team upload.

  [ John Doe ]
  * Some change

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test-pkg", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_author_not_uploader() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Maintainer: Team <team@example.com>
Uploaders: Someone Else <other@example.com>
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let changelog_content = r#"test-pkg (1.0-2) UNRELEASED; urgency=medium

  * Team upload.

  [ John Doe ]
  * Some change

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test-pkg", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
