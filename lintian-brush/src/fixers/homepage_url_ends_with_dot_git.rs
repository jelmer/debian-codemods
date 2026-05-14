use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DESCRIPTION: &str = "Remove .git suffix from Homepage URL.";
const LABEL: &str = "Remove .git suffix from Homepage URL.";

/// Check if a URL path ends with .git
fn should_fix_url(url_str: &str) -> bool {
    url::Url::parse(url_str)
        .map(|u| u.path().ends_with(".git"))
        .unwrap_or(false)
}

/// Remove .git suffix from URL path, preserving query and fragment
fn fix_url(url_str: &str) -> Option<String> {
    let mut url = url::Url::parse(url_str).ok()?;
    let path = url.path().to_string();
    if !path.ends_with(".git") {
        return None;
    }
    url.set_path(&path[..path.len() - 4]);
    Some(url.to_string())
}

/// Determine which tag applies based on the URL host.
fn get_tag_for_url(url_str: &str) -> Option<&'static str> {
    let url = url::Url::parse(url_str).ok()?;
    let host = url.host_str()?;
    match host {
        "github.com" | "www.github.com" => Some("homepage-github-url-ends-with-dot-git"),
        "gitlab.com" | "www.gitlab.com" => Some("homepage-gitlab-url-ends-with-dot-git"),
        "salsa.debian.org" => Some("homepage-salsa-url-ends-with-dot-git"),
        _ => None,
    }
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let Some(homepage) = source.get("Homepage") else {
        return Ok(Vec::new());
    };
    if !should_fix_url(&homepage) {
        return Ok(Vec::new());
    }
    let Some(tag) = get_tag_for_url(&homepage) else {
        return Ok(Vec::new());
    };
    let Some(new_homepage) = fix_url(&homepage) else {
        return Ok(Vec::new());
    };

    let issue =
        LintianIssue::source_with_info(tag, Visibility::Info, vec![format!("[{}]", homepage)]);
    Ok(vec![Diagnostic::with_actions(
        issue,
        DESCRIPTION,
        LABEL,
        vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Homepage".into(),
            value: new_homepage,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "homepage-url-ends-with-dot-git",
    tags: [
        "homepage-github-url-ends-with-dot-git",
        "homepage-gitlab-url-ends-with-dot-git",
        "homepage-salsa-url-ends-with-dot-git"
    ],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
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
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

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
            fix_url("https://github.com/user/repo.git").as_deref(),
            Some("https://github.com/user/repo")
        );
        assert_eq!(fix_url("https://github.com/user/repo"), None);
        assert_eq!(
            fix_url("https://github.com/user/repo.git#readme").as_deref(),
            Some("https://github.com/user/repo#readme")
        );
        assert_eq!(
            fix_url("https://github.com/user/repo.git?tab=readme").as_deref(),
            Some("https://github.com/user/repo?tab=readme")
        );
        assert_eq!(
            fix_url("https://github.com/user/repo.git?foo=bar#baz").as_deref(),
            Some("https://github.com/user/repo?foo=bar#baz")
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

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, DESCRIPTION);

        assert_eq!(
            fs::read_to_string(debian_dir.join("control")).unwrap(),
            "Source: test-package\nHomepage: https://github.com/user/repo\n\nPackage: test-package\nDescription: Test\n Testing\n",
        );
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

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, DESCRIPTION);

        assert_eq!(
            fs::read_to_string(debian_dir.join("control")).unwrap(),
            "Source: test-package\nHomepage: https://gitlab.com/user/project\n\nPackage: test-package\nDescription: Test\n Testing\n",
        );
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

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, DESCRIPTION);

        assert_eq!(
            fs::read_to_string(debian_dir.join("control")).unwrap(),
            "Source: test-package\nHomepage: https://salsa.debian.org/team/package\n\nPackage: test-package\nDescription: Test\n Testing\n",
        );
    }

    #[test]
    fn test_no_dot_git() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let original = "Source: test-package\nHomepage: https://github.com/user/repo\n\nPackage: test-package\nDescription: Test\n Testing\n";
        fs::write(debian_dir.join("control"), original).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(debian_dir.join("control")).unwrap(),
            original
        );
    }

    #[test]
    fn test_unknown_domain() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let original = "Source: test-package\nHomepage: https://example.com/repo.git\n\nPackage: test-package\nDescription: Test\n Testing\n";
        fs::write(debian_dir.join("control"), original).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(debian_dir.join("control")).unwrap(),
            original
        );
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

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
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

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, DESCRIPTION);

        assert_eq!(
            fs::read_to_string(debian_dir.join("control")).unwrap(),
            "Source: test-package\nHomepage: https://www.github.com/user/repo\n\nPackage: test-package\nDescription: Test\n Testing\n",
        );
    }
}
