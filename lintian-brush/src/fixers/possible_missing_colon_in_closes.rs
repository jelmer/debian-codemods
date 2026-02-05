use crate::{declare_fixer, Certainty, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_changelog::ChangeLog;
use lazy_regex::{regex, Regex};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

lazy_static::lazy_static! {
    static ref DEBBUGS_CLIENT: Mutex<Option<debbugs::blocking::Debbugs>> = Mutex::new(None);
}

/// Check if a bug is valid for the given package
fn valid_bug(package: &str, bug: u32, net_access: bool) -> Option<bool> {
    if !net_access {
        return None;
    }

    let mut client_guard = DEBBUGS_CLIENT.lock().unwrap();

    if client_guard.is_none() {
        let client = debbugs::blocking::Debbugs::default();
        *client_guard = Some(client);
    }

    if let Some(client) = client_guard.as_ref() {
        match client.get_status(&[bug as i32]) {
            Ok(statuses) => {
                // get_status returns a HashMap<i32, BugReport>
                if let Some(status) = statuses.get(&(bug as i32)) {
                    // Check if the bug's package matches
                    return Some(status.package.as_deref() == Some(package));
                }
                Some(false)
            }
            Err(e) => {
                tracing::warn!("Failed to query bug {}: {}", bug, e);
                None
            }
        }
    } else {
        None
    }
}

/// Check a bug and return validity and certainty
fn check_bug(package: &str, bugno: u32, net_access: bool) -> (bool, Certainty) {
    if let Some(valid) = valid_bug(package, bugno, net_access) {
        return (valid, Certainty::Certain);
    }

    // Let's assume valid, but downgrade certainty
    // Check number of digits; upstream projects don't often hit the 5-digit
    // bug numbers that Debian has.
    let num_digits = bugno.to_string().len();
    if num_digits >= 5 {
        (true, Certainty::Likely)
    } else {
        (true, Certainty::Possible)
    }
}

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let changelog_path = base_path.join("debian/changelog");

    if !changelog_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&changelog_path)?;
    let changelog: ChangeLog = content.parse()?;

    let net_access = preferences.net_access.unwrap_or(false);
    let mut overall_certainty = Certainty::Certain;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Regex patterns
    // Match "closes #123"
    let close_colon_re: &Regex = regex!(r"(?i)(?P<closes>closes) #(?P<bug>[0-9]+)");
    // Match "close: #123" (misspelling)
    let close_typo_re: &Regex = regex!(r"(?i)(?P<close>close): #(?P<bug>[0-9]+)");

    // Use iter_changes_by_author to get Change objects that can be mutated
    let changes = debian_changelog::iter_changes_by_author(&changelog);

    for change in changes {
        let package = change.package().unwrap_or_default();

        // Get the bullets for this change
        let bullets = change.split_into_bullets();

        for bullet in bullets {
            let lines = bullet.lines();

            // Combine all lines in the bullet to process
            let combined = lines.join("\n");

            // Skip lines containing "partially closes"
            if combined.to_lowercase().contains("partially closes") {
                continue;
            }

            let original = combined.clone();
            let mut modified = combined.clone();
            let mut issues_to_fix_colon = Vec::new();
            let mut issues_to_fix_typo = Vec::new();

            // First pass: find issues and check if they should be fixed
            // Get the first line number for this bullet (1-indexed)
            let line_num = bullet
                .line_numbers()
                .first()
                .expect("bullet should have line numbers")
                + 1;

            // Check for missing colon in the combined text
            for caps in close_colon_re.captures_iter(&combined) {
                let bugno: u32 = caps["bug"].parse().unwrap_or(0);
                let matched_text = caps[0].to_string();
                let (valid, bug_certainty) = check_bug(&package, bugno, net_access);

                if crate::certainty_sufficient(bug_certainty, preferences.minimum_certainty)
                    && valid
                {
                    let issue = LintianIssue::source_with_info(
                        "possible-missing-colon-in-closes",
                        vec![format!(
                            "{} [usr/share/doc/{}/changelog.Debian.gz:{}]",
                            matched_text, package, line_num
                        )],
                    );

                    if issue.should_fix(base_path) {
                        issues_to_fix_colon.push((matched_text.clone(), bugno, bug_certainty));
                        overall_certainty =
                            crate::min_certainty(&[overall_certainty, bug_certainty])
                                .unwrap_or(overall_certainty);
                        fixed_issues.push(issue);
                    } else {
                        overridden_issues.push(issue);
                    }
                }
            }

            // Check for misspelling in the combined text
            for caps in close_typo_re.captures_iter(&combined) {
                let bugno: u32 = caps["bug"].parse().unwrap_or(0);
                let matched_text = caps[0].to_string();
                let (valid, bug_certainty) = check_bug(&package, bugno, net_access);

                if crate::certainty_sufficient(bug_certainty, preferences.minimum_certainty)
                    && valid
                {
                    let issue = LintianIssue::source_with_info(
                        "misspelled-closes-bug",
                        vec![format!(
                            "{} [usr/share/doc/{}/changelog.Debian.gz:{}]",
                            matched_text, package, line_num
                        )],
                    );

                    if issue.should_fix(base_path) {
                        issues_to_fix_typo.push((matched_text.clone(), bugno, bug_certainty));
                        overall_certainty =
                            crate::min_certainty(&[overall_certainty, bug_certainty])
                                .unwrap_or(overall_certainty);
                        fixed_issues.push(issue);
                    } else {
                        overridden_issues.push(issue);
                    }
                }
            }

            // Second pass: apply fixes only for issues that should be fixed
            if !issues_to_fix_colon.is_empty() {
                modified = close_colon_re
                    .replace_all(&modified, |caps: &regex::Captures| {
                        let closes = &caps["closes"];
                        let bugno: u32 = caps["bug"].parse().unwrap_or(0);
                        format!("{}: #{}", closes, bugno)
                    })
                    .to_string();
            }

            if !issues_to_fix_typo.is_empty() {
                modified = close_typo_re
                    .replace_all(&modified, |caps: &regex::Captures| {
                        let close = &caps["close"];
                        let bugno: u32 = caps["bug"].parse().unwrap_or(0);
                        format!("{}s: #{}", close, bugno)
                    })
                    .to_string();
            }

            if modified != original {
                // Replace the bullet text - split back into lines
                let new_lines: Vec<&str> = modified.split('\n').collect();
                bullet.replace_with(new_lines);
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Write the updated changelog
    fs::write(&changelog_path, changelog.to_string())?;

    // Build result message based on what was fixed
    let has_colon_fixes = fixed_issues
        .iter()
        .any(|i| i.tag.as_deref() == Some("possible-missing-colon-in-closes"));
    let has_typo_fixes = fixed_issues
        .iter()
        .any(|i| i.tag.as_deref() == Some("misspelled-closes-bug"));

    let description = if has_colon_fixes && !has_typo_fixes {
        "Add missing colon in closes line."
    } else if has_typo_fixes && !has_colon_fixes {
        "Fix misspelling of Close ⇒ Closes."
    } else {
        "Fix formatting of bug closes."
    };

    Ok(FixerResult::builder(description)
        .certainty(overall_certainty)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "possible-missing-colon-in-closes",
    tags: ["possible-missing-colon-in-closes", "misspelled-closes-bug"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
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
    fn test_fix_missing_colon() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = r#"test-package (1.0-1) unstable; urgency=medium

  * Initial release. closes #123456

 -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;
        fs::write(&changelog_path, content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            minimum_certainty: Some(Certainty::Possible),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&changelog_path).unwrap();
        assert!(updated_content.contains("closes: #123456"));
        assert!(!updated_content.contains("closes #123456"));
    }

    #[test]
    fn test_fix_misspelling() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = r#"test-package (1.0-1) unstable; urgency=medium

  * Initial release. close: #123456

 -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;
        fs::write(&changelog_path, content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            minimum_certainty: Some(Certainty::Possible),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&changelog_path).unwrap();
        assert!(updated_content.contains("closes: #123456"));
        assert!(!updated_content.contains("close: #123456"));
    }

    #[test]
    fn test_no_change_partially_closes() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = r#"test-package (1.0-1) unstable; urgency=medium

  * Fix partially closes #123456

 -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;
        fs::write(&changelog_path, content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            minimum_certainty: Some(Certainty::Possible),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_case_insensitive() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = r#"test-package (1.0-1) unstable; urgency=medium

  * Initial release. Closes #123456

 -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;
        fs::write(&changelog_path, content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            minimum_certainty: Some(Certainty::Possible),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&changelog_path).unwrap();
        assert!(updated_content.contains("Closes: #123456"));
    }

    #[test]
    fn test_multiple_bugs() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = r#"test-package (1.0-1) unstable; urgency=medium

  * Initial release. closes #123456 and closes #789012

 -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;
        fs::write(&changelog_path, content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            minimum_certainty: Some(Certainty::Possible),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&changelog_path).unwrap();
        assert!(updated_content.contains("closes: #123456"));
        assert!(updated_content.contains("closes: #789012"));
    }
}
