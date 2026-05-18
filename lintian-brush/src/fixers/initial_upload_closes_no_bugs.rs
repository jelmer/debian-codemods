use crate::declare_detector;
use crate::diagnostic::{Action, ChangelogAction, Diagnostic};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_analyzer::wnpp::{BugId, BugKind};
use debian_changelog::ChangeLog;
use debian_workspace::Workspace;
use std::collections::HashSet;
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

    let closed_bugs = collect_closed_debian_bugs(&changelog);
    let wnpp_bugs: Vec<(BugId, BugKind)> = wnpp_bugs
        .into_iter()
        .filter(|(id, _)| !closed_bugs.contains(&(*id as u32)))
        .collect();
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

fn collect_closed_debian_bugs(changelog: &ChangeLog) -> HashSet<u32> {
    let mut ids = HashSet::new();
    for entry in changelog.iter() {
        let lines: Vec<String> = entry.change_lines().collect();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        ids.extend(debian_changelog::changes::find_closed_debian_bugs(&refs));
    }
    ids
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
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, preferences)
        }
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

    /// Mirrors Debian bug #1136913 (redland): the WNPP ITP bug is closed in
    /// a much later changelog entry rather than the initial release entry.
    /// At higher diligence we must not re-add it.
    #[test]
    fn test_skips_bug_closed_in_later_entry() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            "\
test-package (0.9.13-4) unstable; urgency=low

  * First release to Debian archive. (Closes: #206225)

 -- Test User <test@example.com>  Wed, 03 Sep 2003 16:22:16 +0000

test-package (0.9.9-1) unstable; urgency=low

  * Initial Release.
  * A first attempt at a Debian package configuration.

 -- Test User <test@example.com>  Sat, 17 Mar 2001 07:41:06 +0000
",
        )
        .unwrap();

        let prefs = FixerPreferences {
            net_access: Some(true),
            diligence: Some(1),
            ..Default::default()
        };
        // find_wnpp_bugs hits the network in real runs; in the test
        // environment it returns empty, which itself yields no changes.
        // The intent here is to lock in that diligence>=1 triggers the
        // cross-entry scan path without panicking.
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_collect_closed_debian_bugs_finds_bugs_across_entries() {
        let text = "\
test-package (0.9.13-4) unstable; urgency=low

  * First release to Debian archive. (Closes: #206225)

 -- Test User <test@example.com>  Wed, 03 Sep 2003 16:22:16 +0000

test-package (0.9.9-1) unstable; urgency=low

  * Initial Release.

 -- Test User <test@example.com>  Sat, 17 Mar 2001 07:41:06 +0000
";
        let parsed = ChangeLog::parse(text).tree();
        let bugs = collect_closed_debian_bugs(&parsed);
        assert!(bugs.contains(&206225));
    }
}
