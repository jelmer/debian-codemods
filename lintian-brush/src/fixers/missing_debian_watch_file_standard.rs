use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, WatchAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const WATCH_REL: &str = "debian/watch";

/// Version to declare when a watch file has no `version=` line. Format 4 is
/// the most widely supported line-based standard; uscan treats a version-less
/// file as format 1, so the entries are already line-based and prepending the
/// declaration is the minimal fix.
const ADDED_VERSION: u32 = 4;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let watch_file = match ws.parsed_watch() {
        Ok(w) => w,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    // Lintian only fires when the file declares no standard version.
    if watch_file.version_range().is_some() {
        return Ok(Vec::new());
    }

    // A file with no entries is empty or comments-only; there is nothing to
    // version, so back off rather than declaring a standard for content we do
    // not understand.
    if watch_file.entries().next().is_none() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source("missing-debian-watch-file-standard", Visibility::Warning);

    Ok(vec![Diagnostic::with_actions(
        issue,
        "debian/watch does not declare a standard version.".to_string(),
        format!("Set watch file standard version to {}.", ADDED_VERSION),
        vec![Action::Watch(WatchAction::SetVersion {
            file: PathBuf::from(WATCH_REL),
            version: ADDED_VERSION,
        })],
    )
    .with_certainty(Certainty::Confident)])
}

declare_detector! {
    name: "missing-debian-watch-file-standard",
    tags: ["missing-debian-watch-file-standard"],
    triggers: [debian_workspace::Trigger::Watch(
        debian_workspace::WatchAspect::Version,
    )],
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
            Some("test".into()),
            Some(version),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_watch(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("watch"), content).unwrap();
    }

    #[test]
    fn test_adds_version_line() {
        let tmp = TempDir::new().unwrap();
        write_watch(tmp.path(), "https://example.com/foo foo-(.*).tar.gz\n");

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/watch")).unwrap(),
            "version=4\nhttps://example.com/foo foo-(.*).tar.gz\n",
        );
    }

    #[test]
    fn test_preserves_comments() {
        let tmp = TempDir::new().unwrap();
        write_watch(
            tmp.path(),
            "# watch file for foo\nopts=pgpmode=none https://example.com/ foo-(.*).tar.gz\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/watch")).unwrap(),
            "version=4\n# watch file for foo\nopts=pgpmode=none https://example.com/ foo-(.*).tar.gz\n",
        );
    }

    #[test]
    fn test_no_change_when_version_present() {
        let tmp = TempDir::new().unwrap();
        write_watch(
            tmp.path(),
            "version=4\nhttps://example.com/foo foo-(.*).tar.gz\n",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_only_comments() {
        let tmp = TempDir::new().unwrap();
        write_watch(tmp.path(), "# a watch file with no entries yet\n");

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_empty() {
        let tmp = TempDir::new().unwrap();
        write_watch(tmp.path(), "");

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_watch_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_deb822_version_field_not_flagged() {
        let tmp = TempDir::new().unwrap();
        write_watch(
            tmp.path(),
            "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: foo-(.*).tar.gz\n",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
