use crate::declare_detector;
use crate::diagnostic::{Action, ChangelogAction, Diagnostic};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_changelog::parseaddr;
use debian_workspace::Workspace;
use std::path::PathBuf;

const QA_UPLOAD_LINE: &str = "* QA upload.";

/// Return true if the first bullet line mentions a QA or orphan upload, using
/// the same regexes lintian's `nmu` check applies to decide whether the tag
/// fires.
fn mentions_qa(change_lines: &[String]) -> bool {
    let Some(firstline) = change_lines.iter().find(|l| {
        let trimmed = l.trim_start();
        trimmed.starts_with('*')
    }) else {
        return false;
    };
    let lower = firstline.to_lowercase();
    lower.contains("orphan") || lower.contains("qa upload") || lower.contains("qa group upload")
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let Some(maintainer) = source.get("Maintainer") else {
        return Ok(Vec::new());
    };
    let (_, maintainer_email) = parseaddr(maintainer.trim());
    if maintainer_email != "packages@qa.debian.org" {
        return Ok(Vec::new());
    }

    let changelog = match ws.parsed_changelog() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let Some(entry) = changelog.iter().next() else {
        return Ok(Vec::new());
    };

    // Only touch the entry currently being prepared. Editing a released
    // entry would rewrite history for an upload that already happened.
    if entry.is_unreleased() != Some(true) {
        return Ok(Vec::new());
    }

    let Some(version) = entry.version() else {
        return Ok(Vec::new());
    };

    let change_lines: Vec<String> = entry.change_lines().collect();
    if mentions_qa(&change_lines) {
        return Ok(Vec::new());
    }

    let mut new_lines = Vec::with_capacity(change_lines.len() + 1);
    new_lines.push(QA_UPLOAD_LINE.to_string());
    new_lines.extend(change_lines);

    let issue = LintianIssue::source_with_info(
        "no-qa-in-changelog",
        Visibility::Warning,
        vec!["[debian/changelog:1]".to_string()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Mention QA upload in changelog.",
        "Mention QA upload in changelog.",
        vec![Action::Changelog(ChangelogAction::ReplaceEntryChanges {
            file: PathBuf::from("debian/changelog"),
            version: version.to_string(),
            lines: new_lines,
        })],
    )])
}

declare_detector! {
    name: "no-qa-in-changelog",
    tags: ["no-qa-in-changelog"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Maintainer",
        },
        debian_workspace::Trigger::Changelog(debian_workspace::ChangelogAspect::Body),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test-pkg".into()),
            Some(version.clone()),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_pkg(base: &Path, maintainer: &str, changes: &str, distribution: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            format!("Source: test-pkg\nMaintainer: {maintainer}\n"),
        )
        .unwrap();
        fs::write(
            debian.join("changelog"),
            format!(
                "test-pkg (1.0-2) {distribution}; urgency=medium\n\n{changes}\n -- Jane Doe <jane@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n"
            ),
        )
        .unwrap();
    }

    /// The bullet block passed to [`write_pkg`] for the tests below, ending in
    /// a single newline before the trailer separator.
    const ONE_CHANGE: &str = "  * Fix the thing.\n";

    #[test]
    fn test_mentions_qa_detects_variants() {
        assert!(mentions_qa(&["* QA upload.".to_string()]));
        assert!(mentions_qa(&["* QA group upload.".to_string()]));
        assert!(mentions_qa(&["* Orphan this package.".to_string()]));
        assert!(!mentions_qa(&["* Some other change.".to_string()]));
        assert!(!mentions_qa(&[]));
        // lintian only inspects the first bullet, so a later QA mention
        // does not count.
        assert!(!mentions_qa(&[
            "* Some other change.".to_string(),
            "* qa upload".to_string(),
        ]));
    }

    #[test]
    fn test_adds_qa_upload_line() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "Debian QA Group <packages@qa.debian.org>",
            ONE_CHANGE,
            "UNRELEASED",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/changelog")).unwrap(),
            "test-pkg (1.0-2) UNRELEASED; urgency=medium\n\n  * QA upload.\n  * Fix the thing.\n\n -- Jane Doe <jane@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n",
        );
    }

    #[test]
    fn test_no_change_when_qa_already_mentioned() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "Debian QA Group <packages@qa.debian.org>",
            "  * QA upload.\n  * Fix the thing.\n",
            "UNRELEASED",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_not_orphaned() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "Real Maintainer <maint@example.com>",
            ONE_CHANGE,
            "UNRELEASED",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_released() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "Debian QA Group <packages@qa.debian.org>",
            ONE_CHANGE,
            "unstable",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
