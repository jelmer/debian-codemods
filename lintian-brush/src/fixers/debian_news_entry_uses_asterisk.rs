use crate::declare_detector;
use crate::diagnostic::Diagnostic;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;

/// Whether a change line opens an asterisk bullet, mirroring lintian's
/// `^ \s* [*] \s` test against the NEWS entry's Changes block.
fn is_asterisk_bullet(line: &str) -> bool {
    let rest = line.trim_start();
    let mut chars = rest.chars();
    chars.next() == Some('*') && chars.next().is_some_and(|c| c.is_whitespace())
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let package = ws.package().unwrap_or("").to_string();

    let news = match ws.parsed_news() {
        Ok(n) => n,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    // lintian only checks the most recent (top) entry.
    let Some(entry) = news.iter().next() else {
        return Ok(Vec::new());
    };

    // The regexp anchors at the start of the Changes block, so only the
    // first non-blank change line matters; `change_lines` already skips
    // leading blank lines.
    let Some(first_line) = entry.change_lines().next() else {
        return Ok(Vec::new());
    };
    if !is_asterisk_bullet(&first_line) {
        return Ok(Vec::new());
    }

    // lintian points at the entry header line, 1-indexed.
    let line_no = entry.line() + 1;
    let issue = LintianIssue::source_with_info(
        "debian-news-entry-uses-asterisk",
        Visibility::Info,
        vec![format!(
            "[usr/share/doc/{}/NEWS.Debian.gz:{}]",
            &package, line_no
        )],
    );

    // Report only: turning a bulleted list into the prose paragraphs the
    // Developer's Reference recommends needs human judgement, so we flag
    // the issue without proposing an automatic fix.
    Ok(vec![Diagnostic {
        issue: Some(issue),
        message: "NEWS entry uses asterisks for a bulleted list.".to_string(),
        certainty: None,
        patch_name: None,
        plans: Vec::new(),
    }])
}

declare_detector! {
    name: "debian-news-entry-uses-asterisk",
    tags: ["debian-news-entry-uses-asterisk"],
    triggers: [debian_workspace::Trigger::File("debian/NEWS")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn workspace(base: &Path) -> FsWorkspace {
        let version: crate::Version = "1.0".parse().unwrap();
        FsWorkspace::new(base, Some("test-package".to_string()), Some(version))
    }

    fn write_news(base: &Path, contents: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("NEWS"), contents).unwrap();
    }

    #[test]
    fn test_is_asterisk_bullet() {
        assert!(is_asterisk_bullet("* foo"));
        assert!(is_asterisk_bullet("  * foo"));
        assert!(is_asterisk_bullet("*\tfoo"));
        assert!(!is_asterisk_bullet("*foo"));
        assert!(!is_asterisk_bullet("Regular paragraph."));
        assert!(!is_asterisk_bullet("- foo"));
        assert!(!is_asterisk_bullet(""));
        assert!(!is_asterisk_bullet("text * with asterisk"));
    }

    #[test]
    fn test_detects_asterisk_entry() {
        let tmp = TempDir::new().unwrap();
        write_news(
            tmp.path(),
            "test-package (1.0-1) unstable; urgency=low\n\n  * This is a change.\n  * Another change.\n\n -- Joe <joe@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        );

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert_eq!(
            issue.tag.as_deref(),
            Some("debian-news-entry-uses-asterisk")
        );
        assert_eq!(issue.visibility, Some(Visibility::Info));
        assert_eq!(
            issue.info.as_deref(),
            Some("[usr/share/doc/test-package/NEWS.Debian.gz:1]")
        );
        assert!(diags[0].plans.is_empty());
    }

    #[test]
    fn test_ignores_prose_entry() {
        let tmp = TempDir::new().unwrap();
        write_news(
            tmp.path(),
            "test-package (1.0-1) unstable; urgency=low\n\n  This is a regular paragraph describing the change.\n\n -- Joe <joe@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        );

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn test_only_checks_top_entry() {
        let tmp = TempDir::new().unwrap();
        write_news(
            tmp.path(),
            "test-package (2.0-1) unstable; urgency=low\n\n  Prose paragraph for the latest entry.\n\n -- Joe <joe@example.com>  Tue, 02 Jan 2024 00:00:00 +0000\n\ntest-package (1.0-1) unstable; urgency=low\n\n  * An older bulleted entry.\n\n -- Joe <joe@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        );

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn test_reports_top_entry_line_number() {
        let tmp = TempDir::new().unwrap();
        write_news(
            tmp.path(),
            "test-package (2.0-1) unstable; urgency=low\n\n  * Bulleted latest entry.\n\n -- Joe <joe@example.com>  Tue, 02 Jan 2024 00:00:00 +0000\n",
        );

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].issue.as_ref().unwrap().info.as_deref(),
            Some("[usr/share/doc/test-package/NEWS.Debian.gz:1]")
        );
    }

    #[test]
    fn test_no_news_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn test_issue_honours_override() {
        let tmp = TempDir::new().unwrap();
        write_news(
            tmp.path(),
            "test-package (1.0-1) unstable; urgency=low\n\n  * This is a change.\n\n -- Joe <joe@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        );
        fs::create_dir_all(tmp.path().join("debian/source")).unwrap();
        fs::write(
            tmp.path().join("debian/source/lintian-overrides"),
            "debian-news-entry-uses-asterisk [usr/share/doc/test-package/NEWS.Debian.gz:1]\n",
        )
        .unwrap();

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert!(!issue.should_fix(tmp.path()));
    }

    #[test]
    fn test_report_only_yields_no_changes() {
        let tmp = TempDir::new().unwrap();
        write_news(
            tmp.path(),
            "test-package (1.0-1) unstable; urgency=low\n\n  * This is a change.\n\n -- Joe <joe@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        );

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        let result = crate::builtin_fixers::plan_diagnostics(
            tmp.path(),
            &diags,
            &FixerPreferences::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
