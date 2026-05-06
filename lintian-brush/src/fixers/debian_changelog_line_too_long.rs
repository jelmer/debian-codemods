use crate::declare_detector;
use crate::diagnostic::{Action, ChangelogAction, Diagnostic};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_changelog::textwrap::try_rewrap_changes;
use std::path::PathBuf;

const WIDTH: usize = 80;

pub fn detect(
    ws: &dyn FixerWorkspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let package = ws.package().unwrap_or("").to_string();
    let thorough = preferences
        .extra_env
        .as_ref()
        .and_then(|env| env.get("CHANGELOG_THOROUGH"))
        .map(|v| v == "1")
        .unwrap_or(false);

    let changelog_rel = PathBuf::from("debian/changelog");
    let changelog = match ws.parsed_changelog() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let all_changes = debian_changelog::iter_changes_by_author(&changelog);
    if all_changes.is_empty() {
        return Ok(Vec::new());
    }

    let first_version = all_changes[0].version();
    let changes_to_check: Vec<_> = if thorough {
        all_changes.iter().collect()
    } else {
        all_changes
            .iter()
            .filter(|c| c.version() == first_version)
            .collect()
    };

    let mut versions_with_long_lines: Vec<String> = Vec::new();
    let mut issues_for_version: std::collections::HashMap<String, Vec<LintianIssue>> =
        std::collections::HashMap::new();

    for change in changes_to_check {
        let Some(version) = change.version() else {
            continue;
        };
        let lines = change.lines();
        let line_numbers = change.line_numbers();
        for (idx, line) in lines.iter().enumerate() {
            if line.len() > WIDTH {
                let line_no = line_numbers.get(idx).copied().unwrap_or(0) + 1;
                let issue = LintianIssue::source_with_info(
                    "debian-changelog-line-too-long",
                    vec![format!(
                        "[usr/share/doc/{}/changelog.Debian.gz:{}]",
                        &package, line_no
                    )],
                );
                let v = version.to_string();
                if !issues_for_version.contains_key(&v) {
                    versions_with_long_lines.push(v.clone());
                }
                issues_for_version.entry(v).or_default().push(issue);
            }
        }
    }

    if versions_with_long_lines.is_empty() {
        return Ok(Vec::new());
    }

    let entries_to_process: Vec<_> = if thorough {
        changelog.iter().collect()
    } else {
        changelog.iter().take(1).collect()
    };

    let mut diagnostics = Vec::new();
    for entry in entries_to_process {
        let Some(version) = entry.version() else {
            continue;
        };
        let v = version.to_string();
        let Some(issues) = issues_for_version.remove(&v) else {
            continue;
        };

        let change_lines: Vec<String> = entry.change_lines().collect();
        let change_strs: Vec<&str> = change_lines.iter().map(|s| s.as_str()).collect();
        let wrapped: Vec<String> = try_rewrap_changes(change_strs.iter().copied())
            .map_err(|e| FixerError::Other(format!("Failed to rewrap changes: {}", e)))?
            .into_iter()
            .map(|s| s.into_owned())
            .collect();

        if wrapped == change_lines {
            continue;
        }

        // Emit one diagnostic per long line (so override-by-line is
        // possible), all sharing the same rewrap action. Applying any one
        // is enough; the rest are no-ops once the lines are equal.
        let action = Action::Changelog(ChangelogAction::ReplaceEntryChanges {
            file: changelog_rel.clone(),
            version: v.clone(),
            lines: wrapped,
        });
        for issue in issues {
            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!("wrapped\t{}", v),
                vec![action.clone()],
            ));
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let mut versions: Vec<&str> = fixed
        .iter()
        .filter_map(|d| d.message.strip_prefix("wrapped\t"))
        .collect();
    versions.sort();
    versions.dedup();
    if versions.is_empty() {
        "Wrap long lines in changelog entries.".to_string()
    } else {
        format!(
            "Wrap long lines in changelog entries: {}.",
            versions.join(", ")
        )
    }
}

declare_detector! {
    name: "debian-changelog-line-too-long",
    tags: ["debian-changelog-line-too-long"],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path, package: &str) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, package, &version, &FixerPreferences::default())
    }

    #[test]
    fn test_wrap_long_line() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("changelog");
        fs::write(
            &path,
            "blah (2.6.0) unstable; urgency=medium\n\n  * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to somebody who fixed it.\n\n -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800\n",
        )
        .unwrap();

        let result = run_apply(tmp.path(), "blah").unwrap();
        assert_eq!(
            result.description,
            "Wrap long lines in changelog entries: 2.6.0."
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "blah (2.6.0) unstable; urgency=medium\n\n  * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to somebody\n    who fixed it.\n\n -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800\n",
        );
    }

    #[test]
    fn test_no_long_lines() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            "blah (2.6.0) unstable; urgency=medium\n\n  * Short line.\n\n -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800\n",
        )
        .unwrap();

        assert!(matches!(
            run_apply(tmp.path(), "blah"),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_preserves_indentation() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("changelog");
        fs::write(
            &path,
            "blah (2.6.0) unstable; urgency=medium\n\n  * New upstream release.\n   * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to somebody who fixed it.\n\n -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800\n",
        )
        .unwrap();

        run_apply(tmp.path(), "blah").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "blah (2.6.0) unstable; urgency=medium\n\n  * New upstream release.\n   * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to\n     somebody who fixed it.\n\n -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800\n",
        );
    }
}
