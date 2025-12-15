use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use chrono::Datelike;
use debian_changelog::ChangeLog;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let changelog_path = base_path.join("debian/changelog");

    if !changelog_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&changelog_path)?;
    let changelog: ChangeLog = content
        .parse()
        .map_err(|e| FixerError::Other(format!("Failed to parse changelog: {}", e)))?;

    let mut fixed_versions = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for mut entry in changelog.iter() {
        // Get the timestamp string
        let date_str = match entry.timestamp() {
            Some(s) => s,
            None => continue,
        };

        // The format is: "Day, DD Mon YYYY HH:MM:SS +ZZZZ"
        // We need to parse this leniently, ignoring the day-of-week
        // Extract the date parts after the first comma
        let parts: Vec<&str> = date_str.splitn(2, ", ").collect();
        if parts.len() != 2 {
            continue;
        }

        let orig_day_of_week = parts[0];
        let date_time_part = parts[1];

        // Parse using strptime-like format, ignoring the day-of-week
        // Format: "DD Mon YYYY HH:MM:SS +ZZZZ"
        let parsed_date =
            match chrono::DateTime::parse_from_str(date_time_part, "%d %b %Y %H:%M:%S %z") {
                Ok(dt) => dt,
                Err(_) => {
                    // If we can't parse the date, skip it
                    continue;
                }
            };

        // Format the date back with the correct day-of-week
        let new_date_str = parsed_date.to_rfc2822();

        // Extract the day-of-week from the formatted string
        let new_day_of_week = new_date_str.split(',').next().unwrap_or("");

        // Check if the day-of-week changed
        if new_day_of_week != orig_day_of_week {
            let issue = LintianIssue::source_with_info(
                "debian-changelog-has-wrong-day-of-week",
                vec![format!(
                    "{:04}-{:02}-{:02} is a {}",
                    parsed_date.year(),
                    parsed_date.month(),
                    parsed_date.day(),
                    parsed_date.format("%A")
                )],
            );

            if issue.should_fix(base_path) {
                // Update the date - set_datetime takes a DateTime object
                entry.set_datetime(parsed_date);
                if let Some(version) = entry.version() {
                    fixed_versions.push(version.to_string());
                }
                fixed_issues.push(issue);
            } else {
                overridden_issues.push(issue);
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Write back the modified changelog
    fs::write(&changelog_path, changelog.to_string())?;

    let message = if fixed_versions.len() == 1 {
        format!(
            "Fix day-of-week for changelog entry {}.",
            fixed_versions.join(", ")
        )
    } else {
        format!(
            "Fix day-of-week for changelog entries {}.",
            fixed_versions.join(", ")
        )
    };

    Ok(FixerResult::builder(&message)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "debian-changelog-has-wrong-day-of-week",
    tags: ["debian-changelog-has-wrong-day-of-week"],
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
    fn test_fix_wrong_day_of_week() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // April 22, 2018 was a Sunday, not Monday
        let changelog_content = r#"foo (1.0) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 22 Apr 2018 00:58:14 +0000
"#;
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "foo", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&changelog_path).unwrap();
        assert!(updated_content.contains("Sun, 22 Apr 2018"));
        assert!(!updated_content.contains("Mon, 22 Apr 2018"));
    }

    #[test]
    fn test_no_change_when_day_correct() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // April 22, 2018 was a Sunday - this is correct
        let changelog_content = r#"foo (1.0) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Sun, 22 Apr 2018 00:58:14 +0000
"#;
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "foo", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
