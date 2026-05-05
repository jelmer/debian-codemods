use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
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
    let normalize = |bytes: &[u8]| -> Vec<u8> {
        let mut result = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if i + 5 <= bytes.len() && (bytes[i..i + 5].eq_ignore_ascii_case(b"https")) {
                i += 5;
                continue;
            }
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

fn fix_homepage_url(http_url: &str, net_access_allowed: bool) -> Option<String> {
    if !http_url.starts_with("http:") {
        return None;
    }

    let https_url = format!("https:{}", &http_url[5..]);

    if let Ok(url) = url::Url::parse(http_url) {
        if let Some(host) = url.host_str() {
            if KNOWN_HTTPS.contains(&host) {
                return Some(https_url);
            }
        }
    }

    if !net_access_allowed {
        return None;
    }

    match check_urls_equivalent(http_url, &https_url) {
        Ok(true) => Some(https_url),
        Ok(false) => {
            eprintln!("Pages differ between {} and {}", http_url, https_url);
            None
        }
        Err(e) => {
            eprintln!("Error checking URL equivalence: {}", e);
            None
        }
    }
}

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
        eprintln!(
            "HTTPS URL {} redirected back to {}",
            https_url,
            https_response.url()
        );
        return Ok(false);
    }

    let https_contents = https_response.bytes()?;

    Ok(same_page(&http_contents, &https_contents))
}

pub fn detect(
    base_path: &Path,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_path = base_path.join(&control_rel);
    if !control_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&control_path)?;
    let control: Control = match content.parse() {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let Some(homepage) = source.as_deb822().get("Homepage") else {
        return Ok(Vec::new());
    };

    let net_access_allowed = preferences.net_access.unwrap_or(false);
    let Some(new_homepage) = fix_homepage_url(&homepage, net_access_allowed) else {
        return Ok(Vec::new());
    };

    let issue = LintianIssue::source_with_info(
        "homepage-field-uses-insecure-uri",
        vec![homepage.to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Use secure URI in Homepage field.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Homepage".into(),
            value: new_homepage,
        })],
    )])
}

declare_fixer! {
    name: "homepage-field-uses-insecure-uri",
    tags: ["homepage-field-uses-insecure-uri"],
    diagnose: |basedir, _package, _version, preferences| {
        detect(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(
        base: &Path,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, preferences)
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
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: lintian-brush\nHomepage: http://github.com/jelmer/lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        let result = run_apply(tmp.path(), &preferences).unwrap();
        assert_eq!(result.description, "Use secure URI in Homepage field.");

        let content = fs::read_to_string(debian.join("control")).unwrap();
        assert!(content.contains("Homepage: https://github.com/jelmer/lintian-brush"));
        assert!(!content.contains("Homepage: http://github.com"));
    }

    #[test]
    fn test_already_https() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: lintian-brush\nHomepage: https://github.com/jelmer/lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &preferences),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_homepage() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &preferences),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_unknown_domain_no_net_access() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: lintian-brush\nHomepage: http://example.com/project\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &preferences),
            Err(FixerError::NoChanges)
        ));
    }
}
