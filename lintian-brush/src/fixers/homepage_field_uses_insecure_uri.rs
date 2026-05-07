use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;
use std::time::Duration;

const KNOWN_HTTPS: &[&str] = &[
    "github.com",
    "launchpad.net",
    "pypi.python.org",
    "pear.php.net",
    "pecl.php.net",
    "www.bioconductor.org",
    "cran.r-project.org",
    "wiki.debian.org",
];

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

/// Check if two page contents are the same, ignoring protocol differences
pub fn same_page(http_contents: &[u8], https_contents: &[u8]) -> bool {
    // This is a crude way to determine we end up on the same page, but it works.
    // We remove all instances of "http" and "https" to normalize the content
    let normalize = |bytes: &[u8]| -> Vec<u8> {
        let mut result = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            // Check for "https" (case insensitive)
            if i + 5 <= bytes.len() && (bytes[i..i + 5].eq_ignore_ascii_case(b"https")) {
                i += 5;
                continue;
            }
            // Check for "http" (case insensitive)
            if i + 4 <= bytes.len() && (bytes[i..i + 4].eq_ignore_ascii_case(b"http")) {
                i += 4;
                continue;
            }
            result.push(bytes[i]);
            i += 1;
        }
        result
    };

    normalize(http_contents) == normalize(https_contents)
}

/// Compute the HTTPS replacement URL for an insecure HTTP `Homepage`, if we
/// have enough confidence (known-HTTPS host or a successful URL-equivalence
/// check).
fn fix_homepage_url(http_url: &str, net_access_allowed: bool) -> Option<String> {
    if !http_url.starts_with("http:") {
        return None;
    }

    let https_url = format!("https:{}", &http_url[5..]);

    // Trust our hardcoded allow-list without going to the network.
    if let Ok(url) = url::Url::parse(http_url) {
        if let Some(host) = url.host_str() {
            if KNOWN_HTTPS.contains(&host) {
                return Some(https_url);
            }
        }
    }

    // Otherwise we'd need to verify by fetching both URLs. The detector
    // never blocks on the network — `apply()` consumers may but this code
    // path is shared by an LSP host that must respond synchronously, so we
    // bail unless the caller explicitly opted in.
    if !net_access_allowed {
        return None;
    }

    match check_urls_equivalent(http_url, &https_url) {
        Ok(true) => Some(https_url),
        Ok(false) => {
            tracing::debug!("Pages differ between {} and {}", http_url, https_url);
            None
        }
        Err(e) => {
            tracing::debug!("Error checking URL equivalence: {}", e);
            None
        }
    }
}

/// Check if HTTP and HTTPS URLs return equivalent content
fn check_urls_equivalent(
    http_url: &str,
    https_url: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .user_agent("lintian-brush")
        .build()?;

    let http_response = client.get(http_url).send()?;
    let http_contents = http_response.bytes()?;

    let https_response = client.get(https_url).send()?;
    if !https_response.url().as_str().starts_with("https://") {
        // HTTPS redirected back to HTTP — don't treat as a valid replacement.
        return Ok(false);
    }
    let https_contents = https_response.bytes()?;

    Ok(same_page(&http_contents, &https_contents))
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    // Use the raw string rather than the parsed `homepage()`: an HTTP URL
    // that fails url::Url parsing (e.g. with stray spaces) should still be
    // detected as needing the insecure-URI fix.
    let Some(homepage) = source.get("Homepage") else {
        return Ok(Vec::new());
    };

    let net_access_allowed = preferences.net_access.unwrap_or(false);
    let Some(new_homepage) = fix_homepage_url(&homepage, net_access_allowed) else {
        return Ok(Vec::new());
    };

    let issue =
        LintianIssue::source_with_info("homepage-field-uses-insecure-uri", vec![homepage.clone()]);
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Use secure URI in Homepage field.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Homepage".into(),
            value: new_homepage,
        })],
    )])
}

declare_detector! {
    name: "homepage-field-uses-insecure-uri",
    tags: ["homepage-field-uses-insecure-uri"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Homepage",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{DetectorAdapter, TreeFixerWorkspace};
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply_with(
        base: &Path,
        prefs: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, prefs)
    }

    #[test]
    fn test_same_page_identical() {
        let content = b"<html><body>Hello World</body></html>";
        assert!(same_page(content, content));
    }

    #[test]
    fn test_same_page_with_protocol_difference() {
        let http_content = b"<html><body><a href=\"http://example.com\">link</a></body></html>";
        let https_content = b"<html><body><a href=\"https://example.com\">link</a></body></html>";
        assert!(same_page(http_content, https_content));
    }

    #[test]
    fn test_same_page_different() {
        let http_content = b"<html><body>Page 1</body></html>";
        let https_content = b"<html><body>Page 2</body></html>";
        assert!(!same_page(http_content, https_content));
    }

    #[test]
    fn test_github_http_to_https() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: lintian-brush\nHomepage: http://github.com/jelmer/lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        let result = run_apply_with(base_path, &prefs).unwrap();
        assert_eq!(result.description, "Use secure URI in Homepage field.");

        let content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(content.contains("Homepage: https://github.com/jelmer/lintian-brush"));
        assert!(!content.contains("Homepage: http://github.com"));
    }

    #[test]
    fn test_already_https() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: lintian-brush\nHomepage: https://github.com/jelmer/lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply_with(base_path, &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_homepage() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply_with(base_path, &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_unknown_domain_no_net_access() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: lintian-brush\nHomepage: http://example.com/project\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        // Unknown domain without network access shouldn't change anything.
        let prefs = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply_with(base_path, &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_diagnostic_carries_action() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\nHomepage: http://github.com/jelmer/foo\n\nPackage: foo\nDescription: bar\n bar\n",
        )
        .unwrap();

        let ws = TreeFixerWorkspace::new(tmp.path(), "foo", "1.0".parse().unwrap());
        let prefs = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        let diags = detect(&ws, &prefs).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].plans[0].actions.len(), 1);
        assert_eq!(
            diags[0].plans[0].actions[0],
            Action::Deb822(Deb822Action::SetField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Source,
                field: "Homepage".into(),
                value: "https://github.com/jelmer/foo".into(),
            })
        );
    }
}
