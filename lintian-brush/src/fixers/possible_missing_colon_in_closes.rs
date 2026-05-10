use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, ChangelogAction, Diagnostic};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use lazy_regex::{regex, Regex};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

lazy_static::lazy_static! {
    static ref DEBBUGS_CLIENT: Mutex<Option<debbugs::blocking::Debbugs>> = Mutex::new(None);
}

fn valid_bug(package: &str, bug: u32, net_access: bool) -> Option<bool> {
    if !net_access {
        return None;
    }
    let mut client_guard = DEBBUGS_CLIENT.lock().unwrap();
    if client_guard.is_none() {
        *client_guard = Some(debbugs::blocking::Debbugs::default());
    }
    let client = client_guard.as_ref()?;
    match client.get_status(&[bug as i32]) {
        Ok(statuses) => {
            if let Some(status) = statuses.get(&(bug as i32)) {
                return Some(status.package.as_deref() == Some(package));
            }
            Some(false)
        }
        Err(e) => {
            tracing::warn!("Failed to query bug {}: {}", bug, e);
            None
        }
    }
}

fn check_bug(package: &str, bugno: u32, net_access: bool) -> (bool, Certainty) {
    if let Some(valid) = valid_bug(package, bugno, net_access) {
        return (valid, Certainty::Certain);
    }
    let num_digits = bugno.to_string().len();
    if num_digits >= 5 {
        (true, Certainty::Likely)
    } else {
        (true, Certainty::Possible)
    }
}

const TAG_COLON: char = 'C';
const TAG_TYPO: char = 'T';

pub fn detect(
    ws: &dyn FixerWorkspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let changelog_rel = PathBuf::from("debian/changelog");
    let changelog = match ws.parsed_changelog() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let net_access = preferences.net_access.unwrap_or(false);

    // "closes #123" (no colon)
    let close_colon_re: &Regex = regex!(r"(?i)(?P<closes>closes) #(?P<bug>[0-9]+)");
    // "close: #123" (typo: missing trailing s)
    let close_typo_re: &Regex = regex!(r"(?i)(?P<close>close): #(?P<bug>[0-9]+)");

    let mut occurrence_counts: HashMap<(Option<String>, String), usize> = HashMap::new();
    let mut diagnostics = Vec::new();

    for change in debian_changelog::iter_changes_by_author(&changelog) {
        let package = change.package().unwrap_or_default();
        let Some(version) = change.version() else {
            continue;
        };
        let version_str = version.to_string();

        for bullet in change.split_into_bullets() {
            let lines = bullet.lines();
            let combined = lines.join("\n");
            if combined.to_lowercase().contains("partially closes") {
                continue;
            }

            let line_num = bullet
                .line_numbers()
                .first()
                .copied()
                .map(|n| n + 1)
                .unwrap_or(1);

            // First find any matches to decide if we'd fix this bullet.
            let mut new_text = combined.clone();
            let mut to_emit: Vec<(char, String, Certainty)> = Vec::new();

            for caps in close_colon_re.captures_iter(&combined) {
                let bugno: u32 = caps["bug"].parse().unwrap_or(0);
                let matched_text = caps[0].to_string();
                let (valid, bug_certainty) = check_bug(&package, bugno, net_access);
                if crate::certainty_sufficient(bug_certainty, preferences.minimum_certainty)
                    && valid
                {
                    to_emit.push((TAG_COLON, matched_text, bug_certainty));
                }
            }
            for caps in close_typo_re.captures_iter(&combined) {
                let bugno: u32 = caps["bug"].parse().unwrap_or(0);
                let matched_text = caps[0].to_string();
                let (valid, bug_certainty) = check_bug(&package, bugno, net_access);
                if crate::certainty_sufficient(bug_certainty, preferences.minimum_certainty)
                    && valid
                {
                    to_emit.push((TAG_TYPO, matched_text, bug_certainty));
                }
            }

            if to_emit.is_empty() {
                continue;
            }

            // Apply the substitutions to compute the new bullet text.
            let any_colon = to_emit.iter().any(|(t, _, _)| *t == TAG_COLON);
            let any_typo = to_emit.iter().any(|(t, _, _)| *t == TAG_TYPO);
            if any_colon {
                new_text = close_colon_re
                    .replace_all(&new_text, |caps: &regex::Captures| {
                        let closes = &caps["closes"];
                        let bugno: u32 = caps["bug"].parse().unwrap_or(0);
                        format!("{}: #{}", closes, bugno)
                    })
                    .to_string();
            }
            if any_typo {
                new_text = close_typo_re
                    .replace_all(&new_text, |caps: &regex::Captures| {
                        let close = &caps["close"];
                        let bugno: u32 = caps["bug"].parse().unwrap_or(0);
                        format!("{}s: #{}", close, bugno)
                    })
                    .to_string();
            }
            if new_text == combined {
                continue;
            }

            let author = bullet.author().map(|s| s.to_string());
            let key = (author.clone(), combined.clone());
            let occurrence = *occurrence_counts.entry(key.clone()).or_insert(0);
            occurrence_counts.insert(key, occurrence + 1);

            let new_lines: Vec<String> = new_text.split('\n').map(|s| s.to_string()).collect();

            // The single ReplaceBullet action is shared by all
            // diagnostics generated from this bullet.
            let action = Action::Changelog(ChangelogAction::ReplaceBullet {
                file: changelog_rel.clone(),
                version: version_str.clone(),
                author: author.clone(),
                text: combined.clone(),
                occurrence,
                new_lines,
            });

            for (idx, (kind, matched_text, bug_certainty)) in to_emit.into_iter().enumerate() {
                let (tag_name, tag_visibility, description, label) = if kind == TAG_COLON {
                    (
                        "possible-missing-colon-in-closes",
                        Visibility::Error,
                        "Closes line is missing a colon.",
                        "Add missing colon in closes line.",
                    )
                } else {
                    (
                        "misspelled-closes-bug",
                        Visibility::Warning,
                        "Closes line uses misspelled keyword.",
                        "Fix misspelling of Close ⇒ Closes.",
                    )
                };
                let info = format!(
                    "{} [usr/share/doc/{}/changelog.Debian.gz:{}]",
                    matched_text, package, line_num
                );
                let issue = LintianIssue::source_with_info(tag_name, tag_visibility, vec![info]);
                let diag = Diagnostic::with_actions(
                    issue,
                    description,
                    label,
                    if idx == 0 {
                        vec![action.clone()]
                    } else {
                        Vec::new()
                    },
                )
                .with_certainty(bug_certainty);
                diagnostics.push(diag);
            }
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let has_colon = fixed.iter().any(|(d, _)| {
        d.issue.as_ref().and_then(|i| i.tag.as_deref()) == Some("possible-missing-colon-in-closes")
    });
    let has_typo = fixed.iter().any(|(d, _)| {
        d.issue.as_ref().and_then(|i| i.tag.as_deref()) == Some("misspelled-closes-bug")
    });
    if has_colon && !has_typo {
        "Add missing colon in closes line.".to_string()
    } else if has_typo && !has_colon {
        "Fix misspelling of Close ⇒ Closes.".to_string()
    } else {
        "Fix formatting of bug closes.".to_string()
    }
}

declare_detector! {
    name: "possible-missing-colon-in-closes",
    tags: ["possible-missing-colon-in-closes", "misspelled-closes-bug"],
    triggers: [crate::workspace::Trigger::Changelog(
        crate::workspace::ChangelogAspect::Body,
    )],
    cost: crate::workspace::DetectorCost::Network,
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

    fn run_apply(
        base: &Path,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test-package", &version, preferences)
    }

    #[test]
    fn test_no_changelog() {
        let tmp = TempDir::new().unwrap();
        let preferences = FixerPreferences::default();
        assert!(matches!(
            run_apply(tmp.path(), &preferences),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_fix_missing_colon() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let changelog = debian.join("changelog");
        fs::write(
            &changelog,
            "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release. closes #123456\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        )
        .unwrap();
        let preferences = FixerPreferences {
            net_access: Some(false),
            minimum_certainty: Some(Certainty::Possible),
            ..Default::default()
        };

        run_apply(tmp.path(), &preferences).unwrap();
        assert_eq!(
            fs::read_to_string(&changelog).unwrap(),
            "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release. closes: #123456\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        );
    }

    #[test]
    fn test_fix_misspelling() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let changelog = debian.join("changelog");
        fs::write(
            &changelog,
            "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release. close: #123456\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        )
        .unwrap();
        let preferences = FixerPreferences {
            net_access: Some(false),
            minimum_certainty: Some(Certainty::Possible),
            ..Default::default()
        };

        run_apply(tmp.path(), &preferences).unwrap();
        assert_eq!(
            fs::read_to_string(&changelog).unwrap(),
            "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release. closes: #123456\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        );
    }

    #[test]
    fn test_no_change_partially_closes() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            "test-package (1.0-1) unstable; urgency=medium\n\n  * Fix partially closes #123456\n\n -- Test User <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        )
        .unwrap();
        let preferences = FixerPreferences {
            net_access: Some(false),
            minimum_certainty: Some(Certainty::Possible),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &preferences),
            Err(FixerError::NoChanges)
        ));
    }
}
