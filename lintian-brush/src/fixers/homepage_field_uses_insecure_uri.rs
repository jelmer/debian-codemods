use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_control::lossless::Control;
use std::fs;
use std::path::Path;
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

/// Try to fix an HTTP URL to HTTPS
fn fix_homepage_url(http_url: &str, net_access_allowed: bool) -> Option<String> {
    if !http_url.starts_with("http:") {
        return None;
    }

    let https_url = format!("https:{}", &http_url[5..]);

    // Check if the domain is in our known HTTPS list
    if let Ok(url) = url::Url::parse(http_url) {
        if let Some(host) = url.host_str() {
            if KNOWN_HTTPS.contains(&host) {
                return Some(https_url);
            }
        }
    }

    // If network access is not allowed, we can't verify
    if !net_access_allowed {
        return None;
    }

    // Try to fetch both URLs and compare
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

/// Check if HTTP and HTTPS URLs return equivalent content
fn check_urls_equivalent(
    http_url: &str,
    https_url: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .user_agent("lintian-brush")
        .build()?;

    // Fetch HTTP version
    let http_response = client.get(http_url).send()?;
    let http_contents = http_response.bytes()?;

    // Fetch HTTPS version
    let https_response = client.get(https_url).send()?;

    // Check that HTTPS didn't redirect back to HTTP
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

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&control_path)?;
    let control: Control = content.parse().map_err(|_| FixerError::NoChanges)?;

    let mut source = match control.source() {
        Some(s) => s,
        None => return Err(FixerError::NoChanges),
    };

    let homepage = match source.as_deb822().get("Homepage") {
        Some(h) => h,
        None => return Err(FixerError::NoChanges),
    };

    let net_access_allowed = preferences.net_access.unwrap_or(false);

    let new_homepage = match fix_homepage_url(&homepage, net_access_allowed) {
        Some(h) => h,
        None => return Err(FixerError::NoChanges),
    };

    let issue = LintianIssue::source_with_info(
        "homepage-field-uses-insecure-uri",
        vec![homepage.to_string()],
    );

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    let new_homepage_url = url::Url::parse(&new_homepage)
        .map_err(|e| FixerError::Other(format!("Failed to parse URL: {}", e)))?;
    source.set_homepage(&new_homepage_url);

    fs::write(&control_path, control.to_string())?;

    Ok(FixerResult::builder("Use secure URI in Homepage field.")
        .fixed_issue(issue)
        .build())
}

declare_fixer! {
    name: "homepage-field-uses-insecure-uri",
    tags: ["homepage-field-uses-insecure-uri"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, &preferences).unwrap();
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

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
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

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
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

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        // Unknown domain without network access should not change
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
