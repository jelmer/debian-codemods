use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, WatchAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// New URL for a SourceForge watch entry rewritten to use the qa.debian.org
/// redirector. Either `https://sf.net/<project>/` (when the entry keeps a
/// separate filename pattern column) or `https://sf.net/<project>/<regex>`
/// (when the filename regex was embedded in the original URL).
struct Redirect {
    url: String,
}

/// Decide whether `url`/`pattern` reference a SourceForge download server or
/// project page directly, and if so compute the redirector replacement.
///
/// Mirrors lintian's `debian-watch-lacks-sourceforge-redirector` detection.
/// Returns `None` when the entry is not SourceForge, already uses the
/// redirector, or can't be rewritten with confidence.
fn redirect_for(url: &str, pattern: Option<&str>) -> Option<Redirect> {
    // The URL embeds an uscan filename regex (e.g. `foo-(.+)\.tar\.gz`) whose
    // backslashes are not valid URL syntax, so `url::Url` mangles them. Split
    // the raw string by hand to keep the regex intact.
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" && scheme != "ftp" {
        return None;
    }
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, String::new()),
    };
    // Strip any userinfo and port; we only care about the host.
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or(authority)
        .split(':')
        .next()
        .unwrap_or(authority)
        .to_ascii_lowercase();
    let path = path.as_str();

    // Pull the project name and the trailing filename pattern out of the URL
    // depending on which SourceForge URL shape we are looking at.
    let (project, embedded_pattern) = if is_download_host(&host) {
        // Download server, e.g.
        //   downloads.sourceforge.net/project/<project>/...
        //   prdownloads.sourceforge.net/<project>/...
        //   <mirror>.dl.sourceforge.net/sourceforge/<project>/...
        project_from_download_path(path)?
    } else if is_sourceforge_host(&host) {
        if path.starts_with("/project/showfiles.php") {
            // showfiles.php?group_id=... - the project is identified by a
            // numeric group id we can't translate, so bail out.
            return None;
        } else {
            let rest = path.strip_prefix("/projects/")?;
            // projects/<project>/files/...
            project_from_projects_path(rest)?
        }
    } else {
        return None;
    };

    if let Some(filename) = pattern {
        // The entry has a separate filename pattern column; keep it and just
        // point the URL at the redirector.
        if !filename.contains('(') {
            return None;
        }
        Some(Redirect {
            url: format!("https://sf.net/{project}/"),
        })
    } else {
        // No pattern column: the filename regex is the last path component of
        // the URL. Fold it into the redirector URL (the magic
        // `https://sf.net/<project>/<tar-name>-(.+)...` form).
        let filename = embedded_pattern?;
        if !filename.contains('(') {
            return None;
        }
        Some(Redirect {
            url: format!("https://sf.net/{project}/{filename}"),
        })
    }
}

/// Hosts that are SourceForge download servers (matched by lintian's first
/// regex): `(?:.+\.)?dl`, `(?:pr)?downloads?`, `ftp\d?` or `upload`, followed
/// by `.sourceforge.net` or `.sf.net`.
fn is_download_host(host: &str) -> bool {
    let Some(sub) = host
        .strip_suffix(".sourceforge.net")
        .or_else(|| host.strip_suffix(".sf.net"))
    else {
        return false;
    };
    let label = sub.rsplit('.').next().unwrap_or(sub);
    matches!(
        label,
        "dl" | "download" | "downloads" | "prdownload" | "prdownloads" | "upload"
    ) || is_ftp_label(label)
}

fn is_ftp_label(label: &str) -> bool {
    label
        .strip_prefix("ftp")
        .is_some_and(|rest| rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit()))
}

/// `sourceforge.net`, `sf.net` or their `www.` variants.
fn is_sourceforge_host(host: &str) -> bool {
    matches!(
        host,
        "sourceforge.net" | "sf.net" | "www.sourceforge.net" | "www.sf.net"
    )
}

/// Extract `(project, embedded_filename_pattern)` from a download-server path.
///
/// The first path component is either `project`, `projects` or `sourceforge`
/// (a routing prefix used by some mirrors) or, on prdownloads, the project
/// name itself. The filename regex - if present - is the last component
/// containing a capture group.
fn project_from_download_path(path: &str) -> Option<(String, Option<String>)> {
    let mut parts = path.trim_matches('/').split('/').filter(|p| !p.is_empty());
    let first = parts.next()?;
    let project = match first {
        "project" | "projects" | "sourceforge" => parts.next()?,
        other => other,
    };
    let project = project.to_string();
    let embedded = split_embedded_pattern(path);
    Some((project, embedded))
}

/// Extract `(project, embedded_filename_pattern)` from a `projects/<rest>`
/// path where `<rest>` is `<project>/files/...`.
fn project_from_projects_path(rest: &str) -> Option<(String, Option<String>)> {
    let mut parts = rest.split('/').filter(|p| !p.is_empty());
    let project = parts.next()?.to_string();
    let embedded = split_embedded_pattern(rest);
    Some((project, embedded))
}

/// If the final path component looks like a filename regex (contains a capture
/// group), return it. A trailing `/download` segment - as produced by the
/// SourceForge "files" UI - is ignored.
fn split_embedded_pattern(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    let last = trimmed.rsplit('/').next()?;
    let candidate = if last == "download" {
        trimmed.rsplit('/').nth(1)?
    } else {
        last
    };
    if candidate.contains('(') {
        Some(candidate.to_string())
    } else {
        None
    }
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
        let pattern = entry.matching_pattern();
        let Some(redirect) = redirect_for(&url, pattern.as_deref()) else {
            continue;
        };

        let line_no = entry.line() + 1;
        let info = match &pattern {
            Some(p) => format!("{url} {p} [debian/watch:{line_no}]"),
            None => format!("{url} [debian/watch:{line_no}]"),
        };
        let issue = LintianIssue::source_with_info(
            "debian-watch-lacks-sourceforge-redirector",
            Visibility::Warning,
            vec![info],
        );

        let actions: Vec<Action> = vec![Action::Watch(WatchAction::SetEntryUrl {
            file: watch_rel.clone(),
            url: url.clone(),
            new_url: redirect.url.clone(),
        })];

        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "debian/watch references SourceForge directly.",
                "Use the qa.debian.org redirector in debian/watch.",
                actions,
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-watch-lacks-sourceforge-redirector",
    tags: ["debian-watch-lacks-sourceforge-redirector"],
    triggers: [
        debian_workspace::Trigger::Watch(debian_workspace::WatchAspect::Source),
        debian_workspace::Trigger::Watch(debian_workspace::WatchAspect::MatchingPattern),
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
            Some("test".into()),
            Some(version.clone()),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_watch(dir: &TempDir, content: &str) -> std::path::PathBuf {
        let debian = dir.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let watch = debian.join("watch");
        fs::write(&watch, content).unwrap();
        watch
    }

    #[test]
    fn test_is_download_host() {
        assert!(is_download_host("downloads.sourceforge.net"));
        assert!(is_download_host("prdownloads.sourceforge.net"));
        assert!(is_download_host("dl.sourceforge.net"));
        assert!(is_download_host("foo.dl.sourceforge.net"));
        assert!(is_download_host("ftp.sourceforge.net"));
        assert!(is_download_host("ftp4.sf.net"));
        assert!(is_download_host("upload.sourceforge.net"));
        assert!(!is_download_host("sourceforge.net"));
        assert!(!is_download_host("www.sourceforge.net"));
        assert!(!is_download_host("example.com"));
    }

    #[test]
    fn test_redirect_download_server_embedded_pattern() {
        let r = redirect_for(
            "http://downloads.sourceforge.net/project/foo/foo/foo-(.+)\\.tar\\.gz",
            None,
        )
        .unwrap();
        // No pattern column, so the filename regex is folded into the URL.
        assert_eq!(r.url, "https://sf.net/foo/foo-(.+)\\.tar\\.gz");
    }

    #[test]
    fn test_redirect_prdownloads() {
        let r = redirect_for(
            "http://prdownloads.sourceforge.net/bar/bar-(.+)\\.tar\\.gz",
            None,
        )
        .unwrap();
        assert_eq!(r.url, "https://sf.net/bar/bar-(.+)\\.tar\\.gz");
    }

    #[test]
    fn test_redirect_dl_mirror() {
        let r = redirect_for(
            "http://heanet.dl.sourceforge.net/sourceforge/baz/baz-(.+)\\.tar\\.gz",
            None,
        )
        .unwrap();
        assert_eq!(r.url, "https://sf.net/baz/baz-(.+)\\.tar\\.gz");
    }

    #[test]
    fn test_redirect_projects_files_separate_pattern() {
        let r = redirect_for(
            "http://sourceforge.net/projects/foo/files/foo/",
            Some("foo-(.+)\\.tar\\.gz"),
        )
        .unwrap();
        // Separate pattern column is kept, so only the URL changes.
        assert_eq!(r.url, "https://sf.net/foo/");
    }

    #[test]
    fn test_redirect_projects_files_download_suffix() {
        let r = redirect_for(
            "https://sourceforge.net/projects/quux/files/quux-(.+)\\.tar\\.gz/download",
            None,
        )
        .unwrap();
        assert_eq!(r.url, "https://sf.net/quux/quux-(.+)\\.tar\\.gz");
    }

    #[test]
    fn test_showfiles_php_not_rewritten() {
        assert!(redirect_for(
            "http://sourceforge.net/project/showfiles.php?group_id=12345",
            Some("baz-(.+)\\.tar\\.gz"),
        )
        .is_none());
    }

    #[test]
    fn test_non_sourceforge() {
        assert!(redirect_for("http://example.com/foo/foo-(.+)\\.tar\\.gz", None).is_none());
    }

    #[test]
    fn test_already_redirector() {
        assert!(redirect_for("https://sf.net/foo/", Some("foo-(.+)\\.tar\\.gz")).is_none());
    }

    #[test]
    fn test_no_filename_pattern() {
        // Without a filename regex anywhere we can't build a useful redirector.
        assert!(redirect_for("http://sourceforge.net/projects/foo/files/foo/", None).is_none());
    }

    #[test]
    fn test_apply_download_server() {
        let tmp = TempDir::new().unwrap();
        let watch = write_watch(
            &tmp,
            "version=3\nhttp://downloads.sourceforge.net/project/foo/foo/foo-(.+)\\.tar\\.gz\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "version=3\nhttps://sf.net/foo/foo-(.+)\\.tar\\.gz\n",
        );
    }

    #[test]
    fn test_apply_projects_files() {
        let tmp = TempDir::new().unwrap();
        let watch = write_watch(
            &tmp,
            "version=4\nhttp://sourceforge.net/projects/foo/files/foo/ foo-(.+)\\.tar\\.gz\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "version=4\nhttps://sf.net/foo/ foo-(.+)\\.tar\\.gz\n",
        );
    }

    #[test]
    fn test_no_watch_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_for_redirector() {
        let tmp = TempDir::new().unwrap();
        write_watch(&tmp, "version=4\nhttps://sf.net/foo/ foo-(.+)\\.tar\\.gz\n");
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
