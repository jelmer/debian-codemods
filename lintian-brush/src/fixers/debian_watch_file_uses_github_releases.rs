use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, WatchAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let watch_rel = PathBuf::from("debian/watch");
    let watch_file = match ws.parsed_watch() {
        Ok(w) => w,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut diagnostics = Vec::new();
    for entry in watch_file.entries() {
        let url = entry.url();
        if !url.contains("github.com") || !url.contains("/releases") {
            continue;
        }
        let new_url = url.replace("/releases", "/tags");
        diagnostics.push(
            Diagnostic::untagged(
                "debian/watch: Use GitHub /tags rather than /releases page.",
                vec![Action::Watch(WatchAction::SetEntryUrl {
                    file: watch_rel.clone(),
                    url,
                    new_url,
                })],
            )
            .with_certainty(Certainty::Confident),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-watch-file-uses-github-releases",
    tags: ["debian-watch-file-uses-github-releases"],
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
    fn test_replaces_releases_with_tags() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let watch = debian.join("watch");
        fs::write(
            &watch,
            "version=4\nhttps://github.com/jupyter/jupyter_core/releases .*/archive/(.*)\\.tar\\.gz\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "version=4\nhttps://github.com/jupyter/jupyter_core/tags .*/archive/(.*)\\.tar\\.gz\n",
        );
    }

    #[test]
    fn test_no_watch_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_uses_tags() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "version=4\nhttps://github.com/jupyter/jupyter_core/tags .*/archive/(.*)\\.tar\\.gz\n",
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
