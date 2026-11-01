use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const CORRECT_FORMAT_URI: &str =
    "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/";

/// Mirrors the URI patterns lintian's
/// `Lintian::Check::Debian::Copyright::Dep5::find_dep5_version` treats as
/// recognisable. If this returns `true`, lintian does *not* emit
/// `unknown-copyright-format-uri` for the URI (it may emit a different
/// Format-URI tag — insecure, wiki, out-of-date, unversioned — instead).
fn is_known_format_uri(uri: &str) -> bool {
    is_boilerplate(uri)
        || is_wiki(uri)
        || is_dep_net(uri)
        || is_wsvn(uri)
        || is_viewvc(uri)
        || is_canonical(uri)
}

fn is_boilerplate(uri: &str) -> bool {
    uri.contains("VERSIONED_FORMAT_URL") || uri.contains("rev=REVISION")
}

fn is_wiki(uri: &str) -> bool {
    strip_http(uri)
        .map(|rest| rest.starts_with("wiki.debian.org/Proposals/CopyrightFormat"))
        .unwrap_or(false)
}

fn is_dep_net(uri: &str) -> bool {
    let Some(rest) = strip_http(uri) else {
        return false;
    };
    let rest = rest
        .strip_prefix("dep-team.pages.debian.net/deps/dep5")
        .or_else(|| rest.strip_prefix("dep.debian.net/deps/dep5"));
    matches!(rest, Some("") | Some("/"))
}

fn is_wsvn(uri: &str) -> bool {
    strip_http(uri)
        .map(|rest| rest.starts_with("svn.debian.org/wsvn/dep/web/deps/dep5.mdwn"))
        .unwrap_or(false)
}

fn is_viewvc(uri: &str) -> bool {
    let Some(rest) = strip_http(uri) else {
        return false;
    };
    rest.starts_with("svn.debian.org/viewvc/dep/web/deps/dep5.mdwn")
        || rest.starts_with("anonscm.debian.org/viewvc/dep/web/deps/dep5.mdwn")
}

/// `https?://www.debian.org/doc/(packaging-manuals/)?copyright-format/\d+\.\d+/?`
fn is_canonical(uri: &str) -> bool {
    let Some(rest) = strip_http(uri) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix("www.debian.org/doc/") else {
        return false;
    };
    let rest = rest.strip_prefix("packaging-manuals/").unwrap_or(rest);
    let Some(rest) = rest.strip_prefix("copyright-format/") else {
        return false;
    };
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let mut parts = rest.splitn(2, '.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    !major.is_empty()
        && major.chars().all(|c| c.is_ascii_digit())
        && !minor.is_empty()
        && minor.chars().all(|c| c.is_ascii_digit())
}

fn strip_http(uri: &str) -> Option<&str> {
    uri.strip_prefix("https://")
        .or_else(|| uri.strip_prefix("http://"))
}

/// An unknown URI is close enough to the canonical URI to be safely
/// replaced only if it mentions both `debian.org` and `copyright-format`.
/// Everything else is left alone.
fn is_unknown_fixable(uri: &str) -> bool {
    uri.contains("debian.org") && uri.contains("copyright-format")
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let copyright = match ws.parsed_copyright() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };
    let Some(header) = copyright.header() else {
        return Ok(Vec::new());
    };
    let Some(format) = header.as_deb822().get("Format") else {
        return Ok(Vec::new());
    };

    let is_insecure =
        format.starts_with("http://www.debian.org/doc/packaging-manuals/copyright-format/1.0");
    let is_wiki_uri = format.starts_with("http://wiki.debian.org/Proposals/CopyrightFormat");

    // lintian strips `#fragment` before matching known patterns.
    let stripped = format.split_once('#').map(|(h, _)| h).unwrap_or(&format);
    let is_unknown = !is_known_format_uri(stripped) && is_unknown_fixable(stripped);

    if !is_insecure && !is_wiki_uri && !is_unknown {
        return Ok(Vec::new());
    }
    if format == CORRECT_FORMAT_URI {
        return Ok(Vec::new());
    }

    // All diagnostics share the same fix; later actions are idempotent
    // against earlier ones.
    let make_action = || {
        Action::Deb822(Deb822Action::SetField {
            file: copyright_rel.clone(),
            paragraph: ParagraphSelector::CopyrightHeader,
            field: "Format".into(),
            value: CORRECT_FORMAT_URI.into(),
        })
    };

    let mut diagnostics = Vec::new();
    if is_insecure {
        diagnostics.push(Diagnostic::with_actions(
            LintianIssue::source_with_info(
                "insecure-copyright-format-uri",
                Visibility::Pedantic,
                vec![format.clone()],
            ),
            "Insecure copyright file specification URI.",
            "Use secure copyright file specification URI.",
            vec![make_action()],
        ));
    }
    if is_wiki_uri {
        diagnostics.push(Diagnostic::with_actions(
            LintianIssue::source_with_info(
                "wiki-copyright-format-uri",
                Visibility::Pedantic,
                vec![format.clone()],
            ),
            "Wiki copyright file specification URI.",
            "Use secure copyright file specification URI.",
            vec![make_action()],
        ));
    }
    if is_unknown {
        diagnostics.push(Diagnostic::with_actions(
            LintianIssue::source_with_info(
                "unknown-copyright-format-uri",
                Visibility::Pedantic,
                vec![format],
            ),
            "Unknown copyright format URI.",
            "Use correct machine-readable copyright file URI.",
            vec![make_action()],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "copyright-format-uri",
    tags: [
        "insecure-copyright-format-uri",
        "wiki-copyright-format-uri",
        "unknown-copyright-format-uri",
    ],
    // Must convert http to https before adding version (unversioned-copyright-format-uri).
    before: ["unversioned-copyright-format-uri"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Format",
            field: "Format",
        },
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
    fn test_insecure_uri() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: http://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("insecure-copyright-format-uri")
        );
        assert_eq!(
            result.fixed_lintian_issues[0].info.as_deref(),
            Some("http://www.debian.org/doc/packaging-manuals/copyright-format/1.0/")
        );

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n",
        );
    }

    #[test]
    fn test_wiki_uri() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: http://wiki.debian.org/Proposals/CopyrightFormat\nUpstream-Name: test\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        // lintian treats wiki-copyright-format-uri and
        // insecure-copyright-format-uri as mutually exclusive. The wiki
        // branch wins for a wiki URL.
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("wiki-copyright-format-uri")
        );

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n",
        );
    }

    #[test]
    fn test_already_secure() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_is_known_format_uri_canonical() {
        assert!(is_known_format_uri(
            "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"
        ));
        assert!(is_known_format_uri(
            "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0"
        ));
        assert!(is_known_format_uri(
            "http://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"
        ));
        assert!(is_known_format_uri(
            "https://www.debian.org/doc/copyright-format/1.0/"
        ));
    }

    #[test]
    fn test_is_known_format_uri_wiki() {
        assert!(is_known_format_uri(
            "https://wiki.debian.org/Proposals/CopyrightFormat"
        ));
        assert!(is_known_format_uri(
            "http://wiki.debian.org/Proposals/CopyrightFormat?action=recall&rev=174"
        ));
    }

    #[test]
    fn test_is_known_format_uri_dep_net() {
        assert!(is_known_format_uri("http://dep.debian.net/deps/dep5"));
        assert!(is_known_format_uri("http://dep.debian.net/deps/dep5/"));
        assert!(is_known_format_uri(
            "https://dep-team.pages.debian.net/deps/dep5"
        ));
    }

    #[test]
    fn test_is_known_format_uri_svn() {
        assert!(is_known_format_uri(
            "http://svn.debian.org/wsvn/dep/web/deps/dep5.mdwn?op=file&rev=59"
        ));
        assert!(is_known_format_uri(
            "http://anonscm.debian.org/viewvc/dep/web/deps/dep5.mdwn"
        ));
    }

    #[test]
    fn test_is_known_format_uri_unknown() {
        assert!(!is_known_format_uri(
            "https://www.debian.org/copyright-format/1.0/"
        ));
        assert!(!is_known_format_uri("https://example.com/"));
        assert!(!is_known_format_uri(
            "https://www.debian.org/doc/packaging-manuals/copyright-format/"
        ));
        assert!(!is_known_format_uri(
            "https://www.debian.org/doc/packaging-manuals/copyright-format/abc/"
        ));
    }

    #[test]
    fn test_is_unknown_fixable() {
        assert!(is_unknown_fixable(
            "https://www.debian.org/copyright-format/1.0/"
        ));
        assert!(is_unknown_fixable("http://debian.org/copyright-format"));
        assert!(!is_unknown_fixable("https://example.com/some-other"));
        assert!(!is_unknown_fixable("https://debian.org/different-spec"));
    }

    #[test]
    fn test_fixes_unknown_uri_missing_doc_path() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: https://www.debian.org/copyright-format/1.0/\nUpstream-Name: test\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("unknown-copyright-format-uri")
        );
        assert_eq!(
            result.fixed_lintian_issues[0].info.as_deref(),
            Some("https://www.debian.org/copyright-format/1.0/")
        );

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n",
        );
    }

    #[test]
    fn test_no_change_when_unrelated_uri() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: https://example.com/some-licensing-spec\nUpstream-Name: test\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_change_when_boilerplate() {
        // Boilerplate placeholders are handled by boilerplate-copyright-format-uri.
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: <VERSIONED_FORMAT_URL>\nUpstream-Name: test\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_fragment_identifier_is_stripped() {
        // lintian strips `#fragment` before matching; the URI reduces to
        // the canonical form, so nothing should fire.
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/#some-anchor\nUpstream-Name: test\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
