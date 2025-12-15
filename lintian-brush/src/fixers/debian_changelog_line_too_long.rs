use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_changelog::textwrap::try_rewrap_changes;
use debian_changelog::ChangeLog;
use std::fs;
use std::path::Path;

const WIDTH: usize = 80;

pub fn run(base_path: &Path, package: &str, thorough: bool) -> Result<FixerResult, FixerError> {
    let changelog_path = base_path.join("debian/changelog");

    if !changelog_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&changelog_path)?;
    let changelog = content
        .parse::<ChangeLog>()
        .map_err(|e| FixerError::Other(format!("Failed to parse changelog: {}", e)))?;

    // Get all changes with line numbers
    let all_changes = debian_changelog::iter_changes_by_author(&changelog);

    if all_changes.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Only process the first entry's changes unless in thorough mode
    let first_version = all_changes[0].version();
    let changes_to_check: Vec<_> = if thorough {
        all_changes.iter().collect()
    } else {
        all_changes
            .iter()
            .filter(|c| c.version() == first_version)
            .collect()
    };

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut versions_to_fix = std::collections::HashSet::new();

    // Check each change for long lines and create issues inline
    for change in changes_to_check {
        let lines = change.lines();
        let line_numbers = change.line_numbers();

        for (idx, line) in lines.iter().enumerate() {
            if line.len() > WIDTH {
                let line_no = line_numbers.get(idx).copied().unwrap_or(0) + 1; // Convert to 1-indexed
                let issue = LintianIssue::source_with_info(
                    "debian-changelog-line-too-long",
                    vec![format!(
                        "[usr/share/doc/{}/changelog.Debian.gz:{}]",
                        package, line_no
                    )],
                );

                if issue.should_fix(base_path) {
                    fixed_issues.push(issue);
                    if let Some(version) = change.version() {
                        versions_to_fix.insert(version.to_string());
                    }
                } else {
                    overridden_issues.push(issue);
                }
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Now actually rewrap the entries
    let all_entries: Vec<_> = changelog.iter().collect();
    let entries_to_process: Vec<_> = if thorough {
        all_entries
    } else {
        all_entries.into_iter().take(1).collect()
    };

    let mut fixed_versions = Vec::new();

    for entry in entries_to_process {
        // Only rewrap entries whose version has issues that should be fixed
        if let Some(version) = entry.version() {
            if !versions_to_fix.contains(&version.to_string()) {
                continue;
            }
        } else {
            continue; // Skip entries without version
        }

        let change_lines: Vec<String> = entry.change_lines().collect();

        // Rewrap the changes
        let change_strs: Vec<&str> = change_lines.iter().map(|s| s.as_str()).collect();
        let wrapped: Vec<String> = try_rewrap_changes(change_strs.iter().copied())
            .map_err(|e| FixerError::Other(format!("Failed to rewrap changes: {}", e)))?
            .into_iter()
            .map(|s| s.into_owned())
            .collect();

        // Check if anything actually changed
        if wrapped == change_lines {
            continue;
        }

        // Clear existing change lines
        while entry.pop_change_line().is_some() {
            // Keep popping
        }

        // Add the wrapped lines
        for line in wrapped {
            entry.append_change_line(&line);
        }

        match entry.try_version() {
            Some(Ok(version)) => {
                if !fixed_versions.contains(&version.to_string()) {
                    fixed_versions.push(version.to_string());
                }
            }
            None => {
                log::debug!("No version found for changelog entry, skipping version recording.");
            }
            Some(Err(e)) => {
                log::debug!("Failed to parse version for changelog entry: {}", e);
            }
        }
    }

    // Write back the modified changelog
    fs::write(&changelog_path, changelog.to_string())?;

    let description = if !fixed_versions.is_empty() {
        format!(
            "Wrap long lines in changelog entries: {}.",
            fixed_versions.join(", ")
        )
    } else {
        "Wrap long lines in changelog entries.".to_string()
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "debian-changelog-line-too-long",
    tags: ["debian-changelog-line-too-long"],
    apply: |basedir, package, _version, preferences| {
        let thorough = preferences
            .extra_env
            .as_ref()
            .and_then(|env| env.get("CHANGELOG_THOROUGH"))
            .map(|v| v == "1")
            .unwrap_or(false);
        run(basedir, package, thorough)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_wrap_long_line() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let changelog_content = r#"blah (2.6.0) unstable; urgency=medium

  * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to somebody who fixed it.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#;

        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let result = run(temp_dir.path(), "blah", false);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&changelog_path).unwrap();
        // Check that the long line was wrapped
        assert!(updated_content.lines().all(|line| line.len() <= WIDTH));
        assert!(updated_content.contains("Thanks to somebody"));
    }

    #[test]
    fn test_no_long_lines() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let changelog_content = r#"blah (2.6.0) unstable; urgency=medium

  * Short line.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#;

        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let result = run(temp_dir.path(), "blah", false);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_preserves_indentation() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let changelog_content = r#"blah (2.6.0) unstable; urgency=medium

  * New upstream release.
   * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to somebody who fixed it.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#;

        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let result = run(temp_dir.path(), "blah", false);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&changelog_path).unwrap();
        assert!(updated_content.lines().all(|line| line.len() <= WIDTH));
        // Should preserve sub-item indentation
        assert!(updated_content.contains("   *") || updated_content.contains(" * "));
    }
}
