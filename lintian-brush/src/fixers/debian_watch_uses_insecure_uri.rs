use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use std::fs;
use std::path::Path;

const KNOWN_SECURE_HOSTS: &[&str] = &["code.launchpad.net", "launchpad.net", "ftp.gnu.org"];

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let watch_path = base_path.join("debian/watch");

    if !watch_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&watch_path)?;

    let watch_file = debian_watch::parse::parse(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse watch file: {}", e)))?;

    // Check if any entry uses http://
    let has_http = watch_file.entries().any(|entry| {
        let url = entry.url();
        url.starts_with("http://")
    });

    if !has_http {
        return Err(FixerError::NoChanges);
    }

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Modify entries to use https:// for known hosts
    for mut entry in watch_file.entries() {
        let url = entry.url();
        if url.starts_with("http://") {
            let mut new_url = url.clone();

            // Apply stock replacements for known hosts
            for hostname in KNOWN_SECURE_HOSTS {
                let http_url = format!("http://{}/", hostname);
                let https_url = format!("https://{}/", hostname);
                if new_url.contains(&http_url) {
                    new_url = new_url.replace(&http_url, &https_url);
                }
            }

            if new_url != url {
                let line_number = entry.line() + 1;
                let issue = LintianIssue::source_with_info(
                    "debian-watch-uses-insecure-uri",
                    vec![format!("{} [debian/watch:{}]", url, line_number)],
                );

                if issue.should_fix(base_path) {
                    entry.set_url(&new_url);
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Write back the modified watch file
    fs::write(&watch_path, watch_file.to_string())?;

    Ok(FixerResult::builder("Use secure URI in debian/watch.")
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "debian-watch-uses-insecure-uri",
    tags: ["debian-watch-uses-insecure-uri"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_replace_insecure_uri() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content = "version=4\nhttp://ftp.gnu.org/foo/foo-(.*).tar.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&watch_path).unwrap();
        assert!(updated_content.contains("https://ftp.gnu.org/"));
        assert!(!updated_content.contains("http://ftp.gnu.org/"));
    }

    #[test]
    fn test_replace_launchpad_uri() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content = "version=4\nhttp://code.launchpad.net/foo/foo-(.*).tar.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&watch_path).unwrap();
        assert!(updated_content.contains("https://code.launchpad.net/"));
        assert!(!updated_content.contains("http://code.launchpad.net/"));
    }

    #[test]
    fn test_no_change_when_already_https() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content = "version=4\nhttps://ftp.gnu.org/foo/foo-(.*).tar.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_watch_file() {
        let temp_dir = TempDir::new().unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_for_unknown_host() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content = "version=4\nhttp://example.com/foo/foo-(.*).tar.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
