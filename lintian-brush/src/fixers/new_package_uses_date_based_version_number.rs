use crate::{FixerError, FixerResult, LintianIssue, PackageType};
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

    // Check if there's only 1 entry - this is to match the Python behavior
    // which only processes new packages (with a single changelog entry)
    if changelog.iter().count() != 1 {
        return Err(FixerError::NoChanges);
    }

    // Get the first entry
    let mut entry = changelog.iter().next().unwrap();

    // Check if distribution is unreleased
    if entry.is_unreleased() != Some(true) {
        return Err(FixerError::NoChanges);
    }

    // Get upstream version
    let version = entry.version().ok_or(FixerError::NoChanges)?;
    let upstream_version = &version.upstream_version;

    // Check if it matches the pattern: exactly 8 digits starting with 2
    if upstream_version.len() != 8 {
        return Err(FixerError::NoChanges);
    }

    if !upstream_version.starts_with('2') {
        return Err(FixerError::NoChanges);
    }

    if !upstream_version.chars().all(|c| c.is_ascii_digit()) {
        return Err(FixerError::NoChanges);
    }

    let issue = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        tag: Some("new-package-uses-date-based-version-number".to_string()),
        info: None,
    };

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Prefix the version with "0~"
    let old_version = version.to_string();
    let new_version_str = format!("0~{}", old_version);
    let new_version: debversion::Version = new_version_str
        .parse()
        .map_err(|e| FixerError::Other(format!("Failed to parse new version: {}", e)))?;

    // Modify the entry's version
    entry.set_version(&new_version);

    // Write back the modified changelog
    fs::write(&changelog_path, changelog.to_string())?;

    Ok(
        FixerResult::builder("Use version prefix for date-based versionioning.")
            .fixed_issue(issue)
            .build(),
    )
}

declare_fixer! {
    name: "new-package-uses-date-based-version-number",
    tags: ["new-package-uses-date-based-version-number"],
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
    fn test_prefix_date_based_version() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let changelog_content = r#"foo (20231225) UNRELEASED; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 25 Dec 2023 12:00:00 +0000
"#;
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "foo", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&changelog_path).unwrap();
        assert!(updated_content.contains("(0~20231225)"));
        assert!(!updated_content.contains("foo (20231225)"));
    }

    #[test]
    fn test_no_change_when_not_unreleased() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let changelog_content = r#"foo (20231225) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 25 Dec 2023 12:00:00 +0000
"#;
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "foo", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_not_date_pattern() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let changelog_content = r#"foo (1.0) UNRELEASED; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 25 Dec 2023 12:00:00 +0000
"#;
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "foo", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_multiple_entries() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let changelog_content = r#"foo (20231226) UNRELEASED; urgency=medium

  * Second release.

 -- John Doe <john@example.com>  Tue, 26 Dec 2023 12:00:00 +0000

foo (20231225) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 25 Dec 2023 12:00:00 +0000
"#;
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "foo", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
