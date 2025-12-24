use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_control::lossless::Control;
use std::fs;
use std::path::Path;

/// Check if a URL path ends with .git
fn should_fix_url(url_str: &str) -> bool {
    let url = match url::Url::parse(url_str) {
        Ok(u) => u,
        Err(_) => return false,
    };

    url.path().ends_with(".git")
}

/// Remove .git suffix from URL path, preserving query and fragment
fn fix_url(url_str: &str) -> String {
    let mut url = match url::Url::parse(url_str) {
        Ok(u) => u,
        Err(_) => return url_str.to_string(),
    };

    let path = url.path().to_string();
    if path.ends_with(".git") {
        let new_path = &path[..path.len() - 4];
        url.set_path(new_path);
    }

    url.to_string()
}

/// Determine which tag applies based on the URL
fn get_tag_for_url(url_str: &str) -> Option<&'static str> {
    let url = match url::Url::parse(url_str) {
        Ok(u) => u,
        Err(_) => return None,
    };

    let host = url.host_str()?;

    match host {
        "github.com" | "www.github.com" => Some("homepage-github-url-ends-with-dot-git"),
        "gitlab.com" | "www.gitlab.com" => Some("homepage-gitlab-url-ends-with-dot-git"),
        "salsa.debian.org" => Some("homepage-salsa-url-ends-with-dot-git"),
        _ => None,
    }
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
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

    if !should_fix_url(&homepage) {
        return Err(FixerError::NoChanges);
    }

    let tag = match get_tag_for_url(&homepage) {
        Some(t) => t,
        None => return Err(FixerError::NoChanges),
    };

    let issue = LintianIssue::source_with_info(tag, vec![format!("[{}]", homepage)]);

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    let new_homepage = fix_url(&homepage);

    let new_homepage_url = url::Url::parse(&new_homepage)
        .map_err(|e| FixerError::Other(format!("Failed to parse fixed URL: {}", e)))?;
    source.set_homepage(&new_homepage_url);

    fs::write(&control_path, control.to_string())?;

    Ok(
        FixerResult::builder("Remove .git suffix from Homepage URL.")
            .certainty(crate::Certainty::Certain)
            .fixed_issue(issue)
            .build(),
    )
}

declare_fixer! {
    name: "homepage-url-ends-with-dot-git",
    tags: [
        "homepage-github-url-ends-with-dot-git",
        "homepage-gitlab-url-ends-with-dot-git",
        "homepage-salsa-url-ends-with-dot-git"
    ],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_should_fix_url() {
        assert!(should_fix_url("https://github.com/user/repo.git"));
        assert!(should_fix_url("https://gitlab.com/user/repo.git"));
        assert!(should_fix_url("https://github.com/user/repo.git#readme"));
        assert!(should_fix_url(
            "https://github.com/user/repo.git?tab=readme"
        ));
        assert!(!should_fix_url("https://github.com/user/repo"));
        assert!(!should_fix_url("https://example.com"));
        assert!(!should_fix_url("https://github.com/user/repo#branch"));
    }

    #[test]
    fn test_fix_url() {
        assert_eq!(
            fix_url("https://github.com/user/repo.git"),
            "https://github.com/user/repo"
        );
        assert_eq!(
            fix_url("https://github.com/user/repo"),
            "https://github.com/user/repo"
        );
        // Test with fragment
        assert_eq!(
            fix_url("https://github.com/user/repo.git#readme"),
            "https://github.com/user/repo#readme"
        );
        // Test with query
        assert_eq!(
            fix_url("https://github.com/user/repo.git?tab=readme"),
            "https://github.com/user/repo?tab=readme"
        );
        // Test with both query and fragment
        assert_eq!(
            fix_url("https://github.com/user/repo.git?foo=bar#baz"),
            "https://github.com/user/repo?foo=bar#baz"
        );
    }

    #[test]
    fn test_get_tag_for_url() {
        assert_eq!(
            get_tag_for_url("https://github.com/user/repo.git"),
            Some("homepage-github-url-ends-with-dot-git")
        );
        assert_eq!(
            get_tag_for_url("https://www.github.com/user/repo.git"),
            Some("homepage-github-url-ends-with-dot-git")
        );
        assert_eq!(
            get_tag_for_url("https://gitlab.com/user/repo.git"),
            Some("homepage-gitlab-url-ends-with-dot-git")
        );
        assert_eq!(
            get_tag_for_url("https://www.gitlab.com/user/repo.git"),
            Some("homepage-gitlab-url-ends-with-dot-git")
        );
        assert_eq!(
            get_tag_for_url("https://salsa.debian.org/user/repo.git"),
            Some("homepage-salsa-url-ends-with-dot-git")
        );
        assert_eq!(get_tag_for_url("https://example.com/repo.git"), None);
        assert_eq!(get_tag_for_url("not a url"), None);
    }

    #[test]
    fn test_github_fix() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nHomepage: https://github.com/user/repo.git\n\nPackage: test-package\nDescription: Test\n Testing\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove .git suffix from Homepage URL.");

        let content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(content.contains("Homepage: https://github.com/user/repo"));
        assert!(!content.contains("Homepage: https://github.com/user/repo.git"));
    }

    #[test]
    fn test_gitlab_fix() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nHomepage: https://gitlab.com/user/project.git\n\nPackage: test-package\nDescription: Test\n Testing\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove .git suffix from Homepage URL.");

        let content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(content.contains("Homepage: https://gitlab.com/user/project\n"));
    }

    #[test]
    fn test_salsa_fix() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nHomepage: https://salsa.debian.org/team/package.git\n\nPackage: test-package\nDescription: Test\n Testing\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove .git suffix from Homepage URL.");

        let content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(content.contains("Homepage: https://salsa.debian.org/team/package\n"));
    }

    #[test]
    fn test_no_dot_git() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nHomepage: https://github.com/user/repo\n\nPackage: test-package\nDescription: Test\n Testing\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_unknown_domain() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nHomepage: https://example.com/repo.git\n\nPackage: test-package\nDescription: Test\n Testing\n",
        )
        .unwrap();

        // Unknown domain should not be fixed
        let result = run(base_path);
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
            "Source: test-package\n\nPackage: test-package\nDescription: Test\n Testing\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_www_prefix() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nHomepage: https://www.github.com/user/repo.git\n\nPackage: test-package\nDescription: Test\n Testing\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove .git suffix from Homepage URL.");

        let content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(content.contains("Homepage: https://www.github.com/user/repo\n"));
    }
}
