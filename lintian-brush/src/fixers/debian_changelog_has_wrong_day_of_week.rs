use crate::diagnostic::{Action, ChangelogAction, Diagnostic};
use crate::{FixerError, LintianIssue};
use chrono::Datelike;
use debian_changelog::ChangeLog;
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let changelog_rel = PathBuf::from("debian/changelog");
    let abs = base_path.join(&changelog_rel);
    if !abs.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&abs)?;
    let changelog = ChangeLog::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse changelog: {}", e)))?;

    let mut diagnostics = Vec::new();

    for entry in changelog.iter() {
        let Some(date_str) = entry.timestamp() else {
            continue;
        };
        let Some(version) = entry.version() else {
            continue;
        };

        let parts: Vec<&str> = date_str.splitn(2, ", ").collect();
        if parts.len() != 2 {
            continue;
        }
        let orig_day_of_week = parts[0];
        let date_time_part = parts[1];

        let Ok(parsed_date) =
            chrono::DateTime::parse_from_str(date_time_part, "%d %b %Y %H:%M:%S %z")
        else {
            continue;
        };

        let new_date_str = parsed_date.to_rfc2822();
        let new_day_of_week = new_date_str.split(',').next().unwrap_or("");
        if new_day_of_week == orig_day_of_week {
            continue;
        }

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
        let version_str = version.to_string();
        let action = Action::Changelog(ChangelogAction::SetEntryDate {
            file: changelog_rel.clone(),
            version: version_str.clone(),
            rfc2822: new_date_str,
        });
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("fixed\t{}", version_str),
            vec![action],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let mut versions: Vec<&str> = fixed
        .iter()
        .filter_map(|d| d.message.strip_prefix("fixed\t"))
        .collect();
    versions.sort();
    versions.dedup();
    if versions.len() == 1 {
        format!("Fix day-of-week for changelog entry {}.", versions[0])
    } else {
        format!(
            "Fix day-of-week for changelog entries {}.",
            versions.join(", ")
        )
    }
}

declare_fixer! {
    name: "debian-changelog-has-wrong-day-of-week",
    tags: ["debian-changelog-has-wrong-day-of-week"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    },
    describe: |fixed, actions| {
        describe_aggregate(fixed, actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "foo", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_fix_wrong_day_of_week() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("changelog");
        fs::write(
            &path,
            "foo (1.0) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 22 Apr 2018 00:58:14 +0000\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Fix day-of-week for changelog entry 1.0."
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "foo (1.0) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Sun, 22 Apr 2018 00:58:14 +0000\n",
        );
    }

    #[test]
    fn test_no_change_when_day_correct() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("changelog");
        let content = "foo (1.0) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Sun, 22 Apr 2018 00:58:14 +0000\n";
        fs::write(&path, content).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }
}
