use crate::{Certainty, FixerError, FixerResult, LintianIssue};
use std::fs;
use std::path::Path;

const REPACK_REGEX: &str = r"(dfsg|debian|ds|repack)";

pub fn run(
    base_path: &Path,
    package: &str,
    upstream_version: &str,
    net_access: bool,
) -> Result<FixerResult, FixerError> {
    let watch_path = base_path.join("debian/watch");

    if !watch_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Read debian/changelog to get the package version
    let changelog_path = base_path.join("debian/changelog");
    if !changelog_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let changelog_content = fs::read_to_string(&changelog_path)?;
    let changelog = debian_changelog::ChangeLog::read_relaxed(&mut changelog_content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse changelog: {}", e)))?;

    let first_entry = changelog.iter().next().ok_or(FixerError::NoChanges)?;

    let version = first_entry.version().ok_or(FixerError::NoChanges)?;

    // Check if version contains repack markers
    let regex = regex::Regex::new(REPACK_REGEX).unwrap();
    if !regex.is_match(&version.to_string()) {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&watch_path)?;

    let watch_file = debian_watch::parse::parse(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse watch file: {}", e)))?;

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for mut entry in watch_file.entries() {
        // Check if dversionmangle or uversionmangle already exists with repack removal
        let has_dversionmangle = entry.get_option("dversionmangle").is_some();
        let has_uversionmangle = entry.get_option("uversionmangle").is_some();

        // If already has appropriate mangling, skip
        if has_dversionmangle || has_uversionmangle {
            continue;
        }

        let line_number = entry.line() + 1; // Convert to 1-indexed
        let issue = LintianIssue::source_with_info(
            "debian-watch-not-mangling-version",
            vec![format!("{} [debian/watch]", line_number)],
        );

        if !issue.should_fix(base_path) {
            overridden_issues.push(issue);
            continue;
        }

        // Add dversionmangle to remove dfsg/ds/debian/repack suffix
        entry.set_option(debian_watch::WatchOption::Dversionmangle(
            "s/\\+(dfsg|ds|debian|repack)(\\d*)$//".to_string(),
        ));
        fixed_issues.push(issue);
    }

    if fixed_issues.is_empty() {
        if overridden_issues.is_empty() {
            return Err(FixerError::NoChanges);
        } else {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
    }

    fs::write(&watch_path, watch_file.to_string())?;

    let certainty = if net_access {
        match crate::watch::verify_watch_entry_discovers_version(
            &watch_path,
            package,
            upstream_version,
        ) {
            Some(true) => Certainty::Certain,
            Some(false) => Certainty::Likely,
            None => Certainty::Likely,
        }
    } else {
        Certainty::Confident
    };

    Ok(
        FixerResult::builder("Add dversionmangle for repack versioning in debian/watch.")
            .certainty(certainty)
            .fixed_issues(fixed_issues)
            .build(),
    )
}

declare_fixer! {
    name: "debian-watch-not-mangling-version",
    tags: ["debian-watch-not-mangling-version", "debian-watch-file-should-mangle-version"],
    apply: |basedir, package, version, preferences| {
        run(basedir, package, &version.upstream_version.to_string(), preferences.net_access.unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_adds_dversionmangle() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content =
            "version=4\nhttps://github.com/example/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let changelog_content = "example (1.0+dfsg-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let result = run(temp_dir.path(), "test", "1.0", false).unwrap();
        assert_eq!(
            result.description,
            "Add dversionmangle for repack versioning in debian/watch."
        );

        let updated_content = fs::read_to_string(&watch_path).unwrap();
        assert!(updated_content.contains("dversionmangle"));
        assert!(updated_content.contains("dfsg"));
    }

    #[test]
    fn test_no_watch_file() {
        let temp_dir = TempDir::new().unwrap();
        let result = run(temp_dir.path(), "test", "1.0", false);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_version_without_repack() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content =
            "version=4\nhttps://github.com/example/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let changelog_content = "example (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let result = run(temp_dir.path(), "test", "1.0", false);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_has_dversionmangle() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content = "version=4\nopts=dversionmangle=s/\\+dfsg$// https://github.com/example/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let changelog_content = "example (1.0+dfsg-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let result = run(temp_dir.path(), "test", "1.0", false);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
