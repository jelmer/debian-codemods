use crate::declare_detector;
use crate::diagnostic::{Action, Dep3Action, Diagnostic};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use patchkit::quilt::SeriesEntry;
use std::path::PathBuf;

/// Mirror lintian's `not_debian_url`: a value is a valid `Bug-Debian`
/// only when it parses as a URL with an http/https scheme and a host
/// of `bugs.debian.org` or `salsa.debian.org`.
fn is_wrong_bug_debian_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return true;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return true;
    }
    match url.host_str() {
        Some(host) => !matches!(
            host.to_ascii_lowercase().as_str(),
            "bugs.debian.org" | "salsa.debian.org"
        ),
        None => true,
    }
}

/// Try to produce a valid `Bug-Debian` URL from a non-URL value.
///
/// Accepts the shorthand forms also understood by
/// [`dep3::parse_debian_bug_id`]: a bare decimal number or a
/// `#`-prefixed number. Returns `None` for anything else so the fixer
/// never has to guess a hostname.
fn normalize_bug_debian_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Only touch values that have no scheme at all - a URL with the
    // wrong scheme or host might point at a real tracker we can't
    // rewrite blindly.
    if trimmed.contains("://") {
        return None;
    }
    let bug = dep3::parse_debian_bug_id(trimmed)?;
    Some(format!("https://bugs.debian.org/{}", bug))
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let Some(series) = ws.parsed_patches_series()? else {
        return Ok(Vec::new());
    };
    let patches_rel = PathBuf::from("debian/patches");

    let mut diagnostics = Vec::new();
    for entry in &series.entries {
        let SeriesEntry::Patch { name, .. } = entry else {
            continue;
        };
        let patch_rel = patches_rel.join(name);
        let Some((Some(header), _)) = ws.parsed_patch(&patch_rel)? else {
            continue;
        };
        let Some(raw) = header.as_deb822().get("Bug-Debian") else {
            continue;
        };
        if !is_wrong_bug_debian_url(raw.trim()) {
            continue;
        }

        let issue = LintianIssue::source_with_info(
            "invalid-dep3-format-patch-wrong-bug-debian-url",
            Visibility::Info,
            vec![format!("[debian/patches/{}]", name)],
        );

        let diag = if let Some(fixed) = normalize_bug_debian_url(&raw) {
            Diagnostic::with_actions(
                issue,
                "Bug-Debian field in DEP-3 patch header is not a Debian BTS URL.",
                "Rewrite Bug-Debian to a bugs.debian.org URL.",
                vec![Action::Dep3(Dep3Action::SetField {
                    file: patch_rel,
                    field: "Bug-Debian".into(),
                    value: fixed,
                })],
            )
            .with_certainty(Certainty::Certain)
        } else {
            Diagnostic {
                issue: Some(issue),
                message: "Bug-Debian field in DEP-3 patch header is not a Debian BTS or salsa URL."
                    .to_string(),
                certainty: None,
                patch_name: None,
                plans: Vec::new(),
            }
        };
        diagnostics.push(diag);
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "invalid-dep3-format-patch-wrong-bug-debian-url",
    tags: ["invalid-dep3-format-patch-wrong-bug-debian-url"],
    triggers: [
        debian_workspace::Trigger::File("debian/patches/series"),
        debian_workspace::Trigger::Glob("debian/patches/*"),
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
            Some(version),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_patch(base: &Path, bug_debian: &str) -> PathBuf {
        let patches = base.join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "fix.patch\n").unwrap();
        let patch = patches.join("fix.patch");
        fs::write(
            &patch,
            format!(
                "Description: Fix a typo\nBug-Debian: {}\n\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-teh\n+the\n",
                bug_debian
            ),
        )
        .unwrap();
        patch
    }

    #[test]
    fn test_is_wrong_bug_debian_url() {
        assert!(!is_wrong_bug_debian_url("https://bugs.debian.org/927914"));
        assert!(!is_wrong_bug_debian_url("http://bugs.debian.org/927914"));
        assert!(!is_wrong_bug_debian_url(
            "https://salsa.debian.org/foo/bar/-/issues/1"
        ));
        assert!(!is_wrong_bug_debian_url("https://BUGS.DEBIAN.ORG/42"));
        assert!(is_wrong_bug_debian_url("#927914"));
        assert!(is_wrong_bug_debian_url("927914"));
        assert!(is_wrong_bug_debian_url("https://example.com/42"));
        assert!(is_wrong_bug_debian_url("ftp://bugs.debian.org/42"));
        assert!(is_wrong_bug_debian_url(""));
    }

    #[test]
    fn test_normalize_bug_debian_url() {
        assert_eq!(
            normalize_bug_debian_url("#927914"),
            Some("https://bugs.debian.org/927914".to_string())
        );
        assert_eq!(
            normalize_bug_debian_url("927914"),
            Some("https://bugs.debian.org/927914".to_string())
        );
        assert_eq!(
            normalize_bug_debian_url("  #42  "),
            Some("https://bugs.debian.org/42".to_string())
        );
        // Fully-qualified URLs pointing somewhere else are too risky to
        // rewrite blindly.
        assert_eq!(normalize_bug_debian_url("https://example.com/42"), None);
        assert_eq!(normalize_bug_debian_url("ftp://bugs.debian.org/42"), None);
        assert_eq!(normalize_bug_debian_url("not a bug"), None);
        assert_eq!(normalize_bug_debian_url(""), None);
    }

    #[test]
    fn test_rewrites_hash_bug_number() {
        let tmp = TempDir::new().unwrap();
        let patch = write_patch(tmp.path(), "#927914");
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&patch).unwrap(),
            "Description: Fix a typo\nBug-Debian: https://bugs.debian.org/927914\n\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-teh\n+the\n",
        );
    }

    #[test]
    fn test_rewrites_bare_bug_number() {
        let tmp = TempDir::new().unwrap();
        let patch = write_patch(tmp.path(), "927914");
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&patch).unwrap(),
            "Description: Fix a typo\nBug-Debian: https://bugs.debian.org/927914\n\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-teh\n+the\n",
        );
    }

    #[test]
    fn test_no_changes_when_already_valid() {
        let tmp = TempDir::new().unwrap();
        write_patch(tmp.path(), "https://bugs.debian.org/927914");
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_salsa_url() {
        let tmp = TempDir::new().unwrap();
        write_patch(tmp.path(), "https://salsa.debian.org/foo/bar/-/issues/1");
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_wrong_host_url() {
        // Reports the issue but has no fix (we can't guess the Debian bug).
        let tmp = TempDir::new().unwrap();
        write_patch(tmp.path(), "https://example.com/42");
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_no_bug_debian() {
        let tmp = TempDir::new().unwrap();
        let patches = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "fix.patch\n").unwrap();
        fs::write(
            patches.join("fix.patch"),
            "Description: Fix a typo\nAuthor: jane@example.com\n\n--- a/file.txt\n+++ b/file.txt\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_series_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
