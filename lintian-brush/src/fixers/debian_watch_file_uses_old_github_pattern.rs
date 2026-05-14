use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, WatchAction};
use crate::{Certainty, FixerError, FixerPreferences};
use debian_workspace::Workspace;
use std::path::PathBuf;

const MESSAGE: &str = "Update pattern for GitHub archive URLs from /<org>/<repo>/tags page/<org>/<repo>/archive/<tag> → /<org>/<repo>/archive/refs/tags/<tag>.";

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let watch_rel = PathBuf::from("debian/watch");
    let watch_file = match ws.parsed_watch() {
        Ok(w) => w,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };

    let mut diagnostics = Vec::new();
    for entry in watch_file.entries() {
        let url = entry.url();
        if !url.contains("github.com") {
            continue;
        }
        let Some(pattern) = entry.matching_pattern() else {
            continue;
        };
        if !pattern.contains("/archive/") || pattern.contains("/archive/refs/tags/") {
            continue;
        }
        let new_pattern = pattern.replace("/archive/", "/archive/refs/tags/");
        diagnostics.push(
            Diagnostic::untagged(
                "debian/watch uses old GitHub archive pattern.",
                MESSAGE,
                vec![Action::Watch(WatchAction::SetEntryMatchingPattern {
                    file: watch_rel.clone(),
                    url: url.clone(),
                    new_pattern,
                })],
            )
            .with_certainty(Certainty::Confident),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-watch-file-uses-old-github-pattern",
    tags: ["debian-watch-file-uses-old-github-pattern"],
    triggers: [
        debian_workspace::Trigger::Watch(debian_workspace::WatchAspect::Source),
        debian_workspace::Trigger::Watch(debian_workspace::WatchAspect::MatchingPattern),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_updates_old_github_pattern() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let watch = debian.join("watch");
        fs::write(
            &watch,
            "version=4\nhttps://github.com/jupyter/jupyter_core/tags .*/archive/(.*)\\.tar\\.gz\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "version=4\nhttps://github.com/jupyter/jupyter_core/tags .*/archive/refs/tags/(.*)\\.tar\\.gz\n",
        );
    }

    #[test]
    fn test_no_watch_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_updated() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "version=4\nhttps://github.com/jupyter/jupyter_core/tags .*/archive/refs/tags/(.*)\\.tar\\.gz\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_non_github() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "version=4\nhttps://example.com/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
