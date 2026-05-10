use crate::declare_detector;
use crate::diagnostic::{Action, ChangelogAction, Diagnostic};
use debian_workspace::Workspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_analyzer::wnpp::{BugId, BugKind};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    if !preferences.net_access.unwrap_or(false) {
        return Ok(Vec::new());
    }

    let changelog = match ws.parsed_changelog() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let Some(last_entry) = changelog.iter().last() else {
        return Ok(Vec::new());
    };

    // If the entry already mentions Closes anywhere, nothing to do.
    let has_closes = last_entry.change_lines().any(|line| {
        let lower = line.to_lowercase();
        lower.contains("closes:") || lower.contains("closes #")
    });
    if has_closes {
        return Ok(Vec::new());
    }

    let Some(package_name) = last_entry.package() else {
        return Ok(Vec::new());
    };
    let Some(version) = last_entry.version() else {
        return Ok(Vec::new());
    };

    // Find the bullet that mentions "Initial release".
    let initial_bullet = last_entry
        .change_lines()
        .find(|l| l.to_lowercase().contains("initial release"));
    let Some(bullet) = initial_bullet else {
        return Ok(Vec::new());
    };

    let Ok(wnpp_bugs) = find_wnpp_bugs(&package_name) else {
        return Ok(Vec::new());
    };
    if wnpp_bugs.is_empty() {
        return Ok(Vec::new());
    }

    let trimmed = bullet.trim_end();
    let mut new_line = if trimmed.ends_with('.') {
        trimmed.to_string()
    } else {
        format!("{}.", trimmed)
    };
    let bug_numbers: Vec<String> = wnpp_bugs.iter().map(|(id, _)| id.to_string()).collect();
    new_line.push_str(&format!(" Closes: #{}", bug_numbers.join(", #")));

    let mut bug_kinds: Vec<String> = wnpp_bugs
        .iter()
        .map(|(_, kind)| format!("{:?}", kind))
        .collect();
    bug_kinds.sort();
    bug_kinds.dedup();

    let issue = LintianIssue::source("initial-upload-closes-no-bugs", Visibility::Warning);
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Initial upload closes no bugs.",
        format!("Add {} bugs in {}.", bug_kinds.join(", "), version),
        vec![Action::Changelog(ChangelogAction::ReplaceBullet {
            file: PathBuf::from("debian/changelog"),
            version: version.to_string(),
            author: None,
            text: bullet,
            occurrence: 0,
            new_lines: vec![new_line],
        })],
    )])
}

fn find_wnpp_bugs(package_name: &str) -> Result<Vec<(BugId, BugKind)>, FixerError> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| FixerError::Other(format!("Failed to create async runtime: {}", e)))?;
    rt.block_on(async {
        match debian_analyzer::wnpp::find_wnpp_bugs_harder(&[package_name]).await {
            Ok(bugs) => Ok(bugs),
            Err(e) => {
                tracing::debug!("Failed to query WNPP bugs: {}", e);
                Ok(Vec::new())
            }
        }
    })
}

declare_detector! {
    name: "initial-upload-closes-no-bugs",
    tags: ["initial-upload-closes-no-bugs"],
    triggers: [
        debian_workspace::Trigger::Changelog(debian_workspace::ChangelogAspect::Version),
        debian_workspace::Trigger::Changelog(debian_workspace::ChangelogAspect::Body),
    ],
    cost: crate::detector::DetectorCost::Network,
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(
        base: &Path,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, preferences)
    }

    #[test]
    fn test_no_changelog() {
        let tmp = TempDir::new().unwrap();
        let prefs = FixerPreferences {
            net_access: Some(true),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_already_has_bugs_closed() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release. Closes: #123456\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            net_access: Some(true),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_already_has_bugs_closed_lowercase() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release (closes: #123456).\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            net_access: Some(true),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_net_access() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_changelog_with_closes_in_different_line() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n  * Closes: #999999\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            net_access: Some(true),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }
}
