use crate::declare_detector;
use crate::diagnostic::{Action, ChangelogAction, Diagnostic};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let changelog = match ws.parsed_changelog() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    // Only act on new packages (single changelog entry).
    if changelog.iter().count() != 1 {
        return Ok(Vec::new());
    }
    let Some(entry) = changelog.iter().next() else {
        return Ok(Vec::new());
    };
    if entry.is_unreleased() != Some(true) {
        return Ok(Vec::new());
    }
    let Some(version) = entry.version() else {
        return Ok(Vec::new());
    };
    let upstream_version = &version.upstream_version;
    if upstream_version.len() != 8
        || !upstream_version.starts_with('2')
        || !upstream_version.chars().all(|c| c.is_ascii_digit())
    {
        return Ok(Vec::new());
    }

    let issue = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        tag: Some("new-package-uses-date-based-version-number".to_string()),
        info: None,
    };

    let old_version = version.to_string();
    let new_version = format!("0~{}", old_version);

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Use version prefix for date-based versionioning.",
        vec![Action::Changelog(ChangelogAction::SetEntryVersion {
            file: PathBuf::from("debian/changelog"),
            version: old_version,
            new_version,
        })],
    )])
}

declare_detector! {
    name: "new-package-uses-date-based-version-number",
    tags: ["new-package-uses-date-based-version-number"],
    triggers: [crate::workspace::Trigger::Changelog(
        crate::workspace::ChangelogAspect::Version,
    )],
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
        adapter.apply(base, "foo", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_prefix_date_based_version() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let changelog = debian.join("changelog");
        fs::write(
            &changelog,
            "foo (20231225) UNRELEASED; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 25 Dec 2023 12:00:00 +0000\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&changelog).unwrap(),
            "foo (0~20231225) UNRELEASED; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 25 Dec 2023 12:00:00 +0000\n",
        );
    }

    #[test]
    fn test_no_change_when_not_unreleased() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            "foo (20231225) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 25 Dec 2023 12:00:00 +0000\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_not_date_pattern() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            "foo (1.0) UNRELEASED; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 25 Dec 2023 12:00:00 +0000\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_multiple_entries() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            "foo (20231226) UNRELEASED; urgency=medium\n\n  * Second release.\n\n -- John Doe <john@example.com>  Tue, 26 Dec 2023 12:00:00 +0000\n\nfoo (20231225) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 25 Dec 2023 12:00:00 +0000\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
