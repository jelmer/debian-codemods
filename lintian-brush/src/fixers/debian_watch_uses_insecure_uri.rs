use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::PathBuf;

const KNOWN_SECURE_HOSTS: &[&str] = &["code.launchpad.net", "launchpad.net", "ftp.gnu.org"];

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rel = PathBuf::from("debian/watch");
    let watch_file = match ws.parsed_watch() {
        Ok(w) => w,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };

    let mut diagnostics = Vec::new();
    let mut seen_substitutions: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();

    for entry in watch_file.entries() {
        let url = entry.url();
        if !url.starts_with("http://") {
            continue;
        }
        let mut new_url = url.clone();
        for hostname in KNOWN_SECURE_HOSTS {
            let http_url = format!("http://{}/", hostname);
            let https_url = format!("https://{}/", hostname);
            if new_url.contains(&http_url) {
                new_url = new_url.replace(&http_url, &https_url);
            }
        }
        if new_url == url {
            continue;
        }

        let line_number = entry.line() + 1;
        let issue = LintianIssue::source_with_info(
            "debian-watch-uses-insecure-uri",
            Visibility::Info,
            vec![format!("{} [debian/watch:{}]", url, line_number)],
        );

        let mut actions = Vec::new();
        for hostname in KNOWN_SECURE_HOSTS {
            let http_url = format!("http://{}/", hostname);
            let https_url = format!("https://{}/", hostname);
            if !url.contains(&http_url) {
                continue;
            }
            let key = (http_url.clone(), https_url.clone());
            if seen_substitutions.insert(key) {
                actions.push(Action::Filesystem(FilesystemAction::Substitute {
                    file: rel.clone(),
                    from: http_url,
                    to: https_url,
                }));
            }
        }

        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "debian/watch uses insecure URI.",
                "Use secure URI in debian/watch.",
                actions,
            )
            .with_certainty(Certainty::Confident),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-watch-uses-insecure-uri",
    tags: ["debian-watch-uses-insecure-uri"],
    triggers: [crate::workspace::Trigger::Watch(
        crate::workspace::WatchAspect::Source,
    )],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_replace_insecure_uri() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let watch = debian.join("watch");
        fs::write(
            &watch,
            "version=4\nhttp://ftp.gnu.org/foo/foo-(.*).tar.gz\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        let content = fs::read_to_string(&watch).unwrap();
        assert!(content.contains("https://ftp.gnu.org/"));
        assert!(!content.contains("http://ftp.gnu.org/"));
    }

    #[test]
    fn test_replace_launchpad_uri() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let watch = debian.join("watch");
        fs::write(
            &watch,
            "version=4\nhttp://code.launchpad.net/foo/foo-(.*).tar.gz\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        let content = fs::read_to_string(&watch).unwrap();
        assert!(content.contains("https://code.launchpad.net/"));
        assert!(!content.contains("http://code.launchpad.net/"));
    }

    #[test]
    fn test_no_change_when_already_https() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "version=4\nhttps://ftp.gnu.org/foo/foo-(.*).tar.gz\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_watch_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_for_unknown_host() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "version=4\nhttp://example.com/foo/foo-(.*).tar.gz\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
