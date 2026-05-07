use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, WatchAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
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
        if !url.contains("githubredir.debian.net") {
            continue;
        }
        let Ok(parsed) = url::Url::parse(&url) else {
            continue;
        };
        if parsed.host_str() != Some("githubredir.debian.net") {
            continue;
        }
        let path_parts: Vec<&str> = parsed.path().trim_matches('/').split('/').collect();
        if path_parts.len() < 3 || path_parts[0] != "github" {
            continue;
        }
        let (org, repo) = (path_parts[1], path_parts[2]);

        let new_url = format!("https://github.com/{}/{}/tags", org, repo);
        let mut actions: Vec<Action> = vec![Action::Watch(WatchAction::SetEntryUrl {
            file: watch_rel.clone(),
            url: url.clone(),
            new_url: new_url.clone(),
        })];
        let matching = entry.matching_pattern().unwrap_or_default();
        if let Some(last_part) = matching.rsplit('/').next() {
            let new_pattern = format!(".*/{}", last_part);
            if new_pattern != matching {
                actions.push(Action::Watch(WatchAction::SetEntryMatchingPattern {
                    file: watch_rel.clone(),
                    url: new_url,
                    new_pattern,
                }));
            }
        }

        let line_no = entry.line() + 1;
        let issue = LintianIssue::source_with_info(
            "debian-watch-file-uses-deprecated-githubredir",
            vec![format!("{} {} [debian/watch:{}]", url, matching, line_no)],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "Remove use of githubredir - see https://lists.debian.org/debian-devel-announce/2014/10/msg00000.html for details.",
                actions,
            )
            .with_certainty(Certainty::Confident),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-watch-file-uses-deprecated-githubredir",
    tags: ["debian-watch-file-uses-deprecated-githubredir"],
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
    fn test_replaces_githubredir() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let watch = debian.join("watch");
        fs::write(
            &watch,
            "version=3\nhttp://githubredir.debian.net/github/developmentseed/mirror http://github.com/developmentseed/mirror/archive/(\\d+.*)\\.tar\\.gz\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "version=3\nhttps://github.com/developmentseed/mirror/tags .*/(\\d+.*)\\.tar\\.gz\n",
        );
    }

    #[test]
    fn test_no_watch_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_githubredir() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "version=4\nhttps://github.com/example/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
