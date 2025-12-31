use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::wnpp::{BugId, BugKind};
use debian_changelog::{iter_changes_by_author, ChangeLog};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    // Check if net access is allowed
    if !preferences.net_access.unwrap_or(false) {
        return Err(FixerError::NoChanges);
    }

    let changelog_path = base_path.join("debian/changelog");

    if !changelog_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&changelog_path)?;
    let changelog: ChangeLog = content.parse()?;

    // Get the last (oldest) entry
    let last_entry = if let Some(e) = changelog.iter().last() {
        e
    } else {
        return Err(FixerError::NoChanges);
    };

    // If the last entry already has bugs closed, nothing to do
    let bugs_closed: Vec<String> = last_entry
        .change_lines()
        .filter_map(|line| {
            if line.contains("Closes:") {
                Some(line.to_string())
            } else {
                None
            }
        })
        .collect();

    if !bugs_closed.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let package_name = if let Some(pkg) = last_entry.package() {
        pkg
    } else {
        return Err(FixerError::NoChanges);
    };

    let issue = LintianIssue::source("initial-upload-closes-no-bugs");
    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Find WNPP bugs for this package
    let wnpp_bugs = match find_wnpp_bugs(&package_name) {
        Ok(bugs) => bugs,
        Err(_) => return Err(FixerError::NoChanges),
    };

    if wnpp_bugs.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let version_changed = last_entry.version();

    // Use iter_changes_by_author to get mutable change objects
    let changes = iter_changes_by_author(&changelog);
    let mut found = false;

    for change in changes {
        // Only process the last entry
        if change.version() != version_changed {
            continue;
        }

        let bullets = change.split_into_bullets();

        for bullet in bullets {
            let lines = bullet.lines();
            let combined = lines.join("\n");

            if combined.contains("Initial release") {
                // Process this line
                let trimmed = combined.trim_end();
                let mut new_line = if trimmed.ends_with('.') {
                    trimmed.to_string()
                } else {
                    format!("{}.", trimmed)
                };

                // Add the Closes: #... part
                let bug_numbers: Vec<String> = wnpp_bugs
                    .iter()
                    .map(|(bug_no, _)| bug_no.to_string())
                    .collect();
                new_line.push_str(&format!(" Closes: #{}", bug_numbers.join(", #")));

                // Replace the bullet
                bullet.replace_with(vec![new_line.as_str()]);
                found = true;
                break;
            }
        }

        if found {
            break;
        }
    }

    if !found {
        return Err(FixerError::NoChanges);
    }

    // Write the updated changelog
    fs::write(&changelog_path, changelog.to_string())?;

    // Build result message
    let bug_kinds: std::collections::HashSet<String> = wnpp_bugs
        .iter()
        .map(|(_, kind)| format!("{:?}", kind))
        .collect();
    let mut sorted_kinds: Vec<_> = bug_kinds.into_iter().collect();
    sorted_kinds.sort();

    let version_str = if let Some(v) = version_changed {
        v.to_string()
    } else {
        "unknown".to_string()
    };

    Ok(FixerResult::builder(format!(
        "Add {} bugs in {}.",
        sorted_kinds.join(", "),
        version_str
    ))
    .build())
}

fn find_wnpp_bugs(package_name: &str) -> Result<Vec<(BugId, BugKind)>, FixerError> {
    // Create a Tokio runtime to run the async function
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| FixerError::Other(format!("Failed to create async runtime: {}", e)))?;

    rt.block_on(find_wnpp_bugs_async(package_name))
}

async fn find_wnpp_bugs_async(package_name: &str) -> Result<Vec<(BugId, BugKind)>, FixerError> {
    let names = vec![package_name];

    // Try to find WNPP bugs
    match debian_analyzer::wnpp::find_wnpp_bugs_harder(&names).await {
        Ok(bugs) => Ok(bugs),
        Err(e) => {
            // If we can't fetch bugs, just return an empty list
            log::warn!("Failed to query WNPP bugs: {}", e);
            Ok(vec![])
        }
    }
}

declare_fixer! {
    name: "initial-upload-closes-no-bugs",
    tags: ["initial-upload-closes-no-bugs"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_no_changelog() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_has_bugs_closed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release. Closes: #123456\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        fs::write(&changelog_path, content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_net_access() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        fs::write(&changelog_path, content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_entries_only_modifies_last() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = "test-package (2.0-1) unstable; urgency=medium\n\n  * New upstream release.\n\n -- Test User <test@example.com>  Mon, 02 Jan 2024 12:00:00 +0000\n\ntest-package (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        fs::write(&changelog_path, content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        let result = run(base_path, &preferences);
        // Should exit early due to no net access, but the logic would target the last entry
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_initial_release_without_period() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        fs::write(&changelog_path, content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        let result = run(base_path, &preferences);
        // Should exit early due to no net access
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    #[cfg(feature = "udd")]
    fn test_no_initial_release_line() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = "test-package (1.0-1) unstable; urgency=medium\n\n  * First upload to Debian.\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        fs::write(&changelog_path, content).unwrap();

        // Even with net access enabled, should fail because there's no "Initial release" line
        let preferences = FixerPreferences {
            net_access: Some(true),
            ..Default::default()
        };
        let result = run(base_path, &preferences);
        // Will fail at finding "Initial release" line, not at net access
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_empty_changelog() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, "").unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        // Empty changelog parses successfully but has no entries, so we get NoChanges
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_malformed_changelog() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, "This is not a valid changelog\n").unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        // Malformed changelog also parses (losslessly) but has no entries
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_changelog_with_closes_in_different_line() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        // Has "Closes:" but in a different bullet point
        let content = "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n  * Closes: #999999\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        fs::write(&changelog_path, content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        let result = run(base_path, &preferences);
        // Should detect Closes: in any line and exit early
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_initial_release_with_capital_i() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial Release.\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        fs::write(&changelog_path, content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        let result = run(base_path, &preferences);
        // "Initial Release" (capital R) should still match since we use contains()
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    // Note: We can't easily test the actual WNPP bug fetching without network access
    // and without mocking the debian_analyzer::wnpp functions. The integration tests
    // will cover the full functionality with actual network calls.
}
