use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, WatchAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// The modern SourceForge URL for a project, which uscan rewrites through the
/// qa.debian.org redirector internally. Keeping the filename pattern in a
/// separate column, so the URL is just the project landing point.
fn sf_net_url(project: &str) -> String {
    format!("https://sf.net/{project}/")
}

/// Detect the deprecated `qa.debian.org/watch/sf.php?...` redirector form and
/// compute the modern `https://sf.net/<project>/` replacement.
///
/// Mirrors lintian's `debian-watch-file-uses-deprecated-sf-redirector-method`
/// detection, which fires on any URL matching `qa.debian.org/watch/sf.php?`.
/// Returns `None` when the URL is not the deprecated form or the project name
/// can't be extracted with confidence.
fn redirect_for(url: &str) -> Option<String> {
    // The URL may embed an uscan filename regex whose backslashes are not
    // valid URL syntax, so split by hand rather than going through url::Url.
    let (_scheme, rest) = url.split_once("://")?;
    let (authority, path_and_query) = match rest.split_once('/') {
        Some((authority, rest)) => (authority, rest),
        None => return None,
    };
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or(authority)
        .split(':')
        .next()
        .unwrap_or(authority)
        .to_ascii_lowercase();
    if host != "qa.debian.org" {
        return None;
    }

    // Only the query form is deprecated by lintian; the path form
    // (sf.php/<project>/...) is what uscan itself generates and is fine.
    let (script_path, query) = path_and_query.split_once('?')?;
    if script_path != "watch/sf.php" {
        return None;
    }

    let project = project_from_query(query)?;
    Some(sf_net_url(&project))
}

/// Extract the `project` parameter from a sf.php query string. Returns `None`
/// if no `project` parameter is present, since without it we can't build a
/// valid SourceForge URL.
fn project_from_query(query: &str) -> Option<String> {
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        if key == "project" && !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let watch_rel = PathBuf::from("debian/watch");
    let watch_file = match ws.parsed_watch() {
        Ok(w) => w,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut diagnostics = Vec::new();
    for entry in watch_file.entries() {
        let url = entry.url();
        let Some(new_url) = redirect_for(&url) else {
            continue;
        };

        let line_no = entry.line() + 1;
        let pattern = entry.matching_pattern();
        let info = match &pattern {
            Some(p) => format!("{url} {p} [debian/watch:{line_no}]"),
            None => format!("{url} [debian/watch:{line_no}]"),
        };
        let issue = LintianIssue::source_with_info(
            "debian-watch-file-uses-deprecated-sf-redirector-method",
            Visibility::Warning,
            vec![info],
        );

        let actions: Vec<Action> = vec![Action::Watch(WatchAction::SetEntryUrl {
            file: watch_rel.clone(),
            url: url.clone(),
            new_url,
        })];

        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "debian/watch uses the deprecated qa.debian.org SourceForge redirector.",
                "Use the sf.net redirector form in debian/watch.",
                actions,
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-watch-file-uses-deprecated-sf-redirector-method",
    tags: ["debian-watch-file-uses-deprecated-sf-redirector-method"],
    triggers: [debian_workspace::Trigger::Watch(
        debian_workspace::WatchAspect::Source,
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

    #[test]
    fn test_redirect_for_query_form() {
        assert_eq!(
            redirect_for("http://qa.debian.org/watch/sf.php?project=foo"),
            Some("https://sf.net/foo/".to_string())
        );
    }

    #[test]
    fn test_redirect_for_extra_params() {
        assert_eq!(
            redirect_for("https://qa.debian.org/watch/sf.php?project=foo-bar&other=1"),
            Some("https://sf.net/foo-bar/".to_string())
        );
    }

    #[test]
    fn test_redirect_for_path_form_ignored() {
        // The path form (no `?`) is what uscan generates; lintian does not
        // flag it, so we leave it alone.
        assert_eq!(
            redirect_for("http://qa.debian.org/watch/sf.php/foo/foo-(.+)\\.tar\\.gz"),
            None
        );
    }

    #[test]
    fn test_redirect_for_no_project() {
        assert_eq!(
            redirect_for("http://qa.debian.org/watch/sf.php?files=foo-(.+)"),
            None
        );
    }

    #[test]
    fn test_redirect_for_other_host() {
        assert_eq!(redirect_for("http://sf.net/foo/foo-(.+)\\.tar\\.gz"), None);
    }

    #[test]
    fn test_redirect_for_empty_project() {
        assert_eq!(
            redirect_for("http://qa.debian.org/watch/sf.php?project="),
            None
        );
    }

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
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
    fn test_replaces_deprecated_redirector() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let watch = debian.join("watch");
        fs::write(
            &watch,
            "version=3\nhttp://qa.debian.org/watch/sf.php?project=foo scripts\\.([\\d.]+)\\.tar\\.gz\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "version=3\nhttps://sf.net/foo/ scripts\\.([\\d.]+)\\.tar\\.gz\n",
        );
    }

    #[test]
    fn test_no_watch_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_path_form_not_touched() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "version=3\nhttp://qa.debian.org/watch/sf.php/foo/foo-(.+)\\.tar\\.gz\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
