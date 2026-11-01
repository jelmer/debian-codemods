use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, SystemdAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Decide whether a Documentation value is a valid URI per lintian's rules.
/// Lintian uses `Data::Validate::URI::is_uri`, which accepts any string that
/// parses as a URI with a scheme. `url::Url::parse` has the same semantics
/// for these inputs.
fn is_valid_uri(value: &str) -> bool {
    url::Url::parse(value).is_ok()
}

/// Try to coerce an invalid Documentation value into a valid URI.
/// Returns `None` if there is no high-confidence correction.
///
/// Only handles the unambiguous case of a bare `www.*` host: there is no
/// reasonable interpretation other than an http(s) URL. Anything else
/// (bare domain, man page shorthand, file path) could be guessed wrong,
/// so we back off.
fn try_fix(value: &str) -> Option<String> {
    if value.starts_with("www.") && !value.contains(char::is_whitespace) {
        let candidate = format!("https://{}", value);
        if is_valid_uri(&candidate) {
            return Some(candidate);
        }
    }
    None
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut entries = match ws.list_dir(Path::new("debian"))? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    entries.sort();

    let mut diagnostics = Vec::new();

    for filename in entries {
        if !filename.ends_with(".service") {
            continue;
        }
        let rel = PathBuf::from("debian").join(&filename);
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let unit = systemd_unit_edit::SystemdUnit::from_str(content).map_err(|e| {
            FixerError::Other(format!("Failed to parse {}: {:?}", rel.display(), e))
        })?;
        let Some(unit_section) = unit.get_section("Unit") else {
            continue;
        };

        let rel_str = rel.to_string_lossy().to_string();

        for raw in unit_section.get_all("Documentation") {
            for token in raw.split_whitespace() {
                if is_valid_uri(token) {
                    continue;
                }

                let issue = LintianIssue::source_with_info(
                    "invalid-systemd-documentation",
                    Visibility::Info,
                    vec![rel_str.clone(), token.to_string()],
                );
                let description = format!("Documentation URI {} is not a valid URI.", token);

                let diagnostic = if let Some(fixed) = try_fix(token) {
                    let actions = vec![
                        Action::Systemd(SystemdAction::RemoveValue {
                            file: rel.clone(),
                            section: "Unit".into(),
                            field: "Documentation".into(),
                            value: token.to_string(),
                        }),
                        Action::Systemd(SystemdAction::Add {
                            file: rel.clone(),
                            section: "Unit".into(),
                            field: "Documentation".into(),
                            value: fixed,
                        }),
                    ];
                    Diagnostic::with_actions(
                        issue,
                        description,
                        format!("Prefix {} with https:// scheme.", token),
                        actions,
                    )
                } else {
                    Diagnostic::with_plans(issue, description, vec![])
                };
                diagnostics.push(diagnostic);
            }
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "invalid-systemd-documentation",
    tags: ["invalid-systemd-documentation"],
    triggers: [debian_workspace::Trigger::Glob("debian/*.service")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
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

    fn run_detect(base: &Path) -> Vec<Diagnostic> {
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version.clone()),
        );
        detect(&ws, &FixerPreferences::default()).unwrap()
    }

    #[test]
    fn is_valid_uri_matches_lintian_semantics() {
        assert!(is_valid_uri("https://example.com"));
        assert!(is_valid_uri("http://www.example.com/path"));
        assert!(is_valid_uri("man:foo(1)"));
        assert!(is_valid_uri("file:/etc/foo.conf"));
        assert!(is_valid_uri("info:bar"));

        assert!(!is_valid_uri("www.example.com"));
        assert!(!is_valid_uri("foo(1)"));
        assert!(!is_valid_uri("/etc/foo.conf"));
        assert!(!is_valid_uri("example.com"));
    }

    #[test]
    fn try_fix_prefixes_www_host_with_https() {
        assert_eq!(
            try_fix("www.example.com"),
            Some("https://www.example.com".to_string())
        );
        assert_eq!(
            try_fix("www.example.com/path/to/docs"),
            Some("https://www.example.com/path/to/docs".to_string())
        );
    }

    #[test]
    fn try_fix_backs_off_for_ambiguous_values() {
        assert_eq!(try_fix("example.com"), None);
        assert_eq!(try_fix("foo(1)"), None);
        assert_eq!(try_fix("/usr/share/doc/foo"), None);
        assert_eq!(try_fix(""), None);
    }

    #[test]
    fn fixes_bare_www_host() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Foo\nDocumentation=www.example.com\n\n[Service]\nExecStart=/bin/true\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Foo\nDocumentation=https://www.example.com\n\n[Service]\nExecStart=/bin/true\n",
        );
    }

    #[test]
    fn preserves_valid_uris_alongside_fix() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDocumentation=man:foo(8) www.example.com\n\n[Service]\nExecStart=/bin/true\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        let out = fs::read_to_string(&path).unwrap();
        assert!(out.contains("man:foo(8)"));
        assert!(out.contains("https://www.example.com"));
        assert!(!out.contains(" www.example.com"));
    }

    #[test]
    fn no_changes_when_only_invalid_is_unfixable() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        let original =
            "[Unit]\nDocumentation=/usr/share/doc/foo/README\n\n[Service]\nExecStart=/bin/true\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn reports_unfixable_invalid_uri_without_a_plan() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("foo.service"),
            "[Unit]\nDocumentation=/usr/share/doc/foo/README\n\n[Service]\nExecStart=/bin/true\n",
        )
        .unwrap();

        let diagnostics = run_detect(tmp.path());
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].plans.is_empty());
        assert_eq!(
            diagnostics[0].issue.as_ref().unwrap().tag.as_deref(),
            Some("invalid-systemd-documentation"),
        );
        assert_eq!(
            diagnostics[0].issue.as_ref().unwrap().info.as_deref(),
            Some("debian/foo.service /usr/share/doc/foo/README"),
        );
    }

    #[test]
    fn no_changes_when_all_valid() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        let original =
            "[Unit]\nDocumentation=man:foo(8) https://example.com/docs\n\n[Service]\nExecStart=/bin/true\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn no_changes_without_unit_section() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(&path, "[Service]\nExecStart=/bin/true\n").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
