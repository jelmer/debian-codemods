use crate::{FixerError, FixerResult, LintianIssue, PackageType};
use debian_analyzer::abstract_control::AbstractSource;
use debian_analyzer::control::TemplatedControlEditor;
use regex::Regex;
use std::path::Path;

/// Fix the VCS Git URL from git://(git|anonscm).debian.org/~user/repo.git
/// to https://anonscm.debian.org/git/users/user/repo.git
pub fn fix_vcs_git_user_url(url: &str) -> Option<String> {
    let re = Regex::new(r"^git://(?:git|anonscm)\.debian\.org/~(.+)$").ok()?;

    if let Some(captures) = re.captures(url) {
        let user_and_path = captures.get(1)?.as_str();
        Some(format!(
            "https://anonscm.debian.org/git/users/{}",
            user_and_path
        ))
    } else {
        None
    }
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    let source = editor.source().ok_or(FixerError::NoChanges)?;

    let vcs_git = match source.get_vcs_url("Git") {
        Some(value) => value,
        None => return Err(FixerError::NoChanges),
    };

    let old_url = vcs_git.clone();

    // Try to fix the URL
    let new_url = match fix_vcs_git_user_url(&vcs_git) {
        Some(url) => url,
        None => return Err(FixerError::NoChanges),
    };

    // Create the issue before modifying
    let issue = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        tag: Some("vcs-git-uses-invalid-user-uri".to_string()),
        info: Some(old_url.clone()),
    };

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Update the Vcs-Git field
    if let Some(mut source) = editor.source() {
        source.set_vcs_url("Git", &new_url);
    }

    editor.commit()?;

    Ok(
        FixerResult::builder("Use valid URI for personal Debian Git repository.")
            .fixed_issue(issue)
            .build(),
    )
}

declare_fixer! {
    name: "vcs-git-uses-invalid-user-uri",
    tags: ["vcs-git-uses-invalid-user-uri"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fix_vcs_git_user_url_git_debian_org() {
        let url = "git://git.debian.org/~user/myproject.git";
        let fixed = fix_vcs_git_user_url(url).unwrap();
        assert_eq!(
            fixed,
            "https://anonscm.debian.org/git/users/user/myproject.git"
        );
    }

    #[test]
    fn test_fix_vcs_git_user_url_anonscm_debian_org() {
        let url = "git://anonscm.debian.org/~jelmer/lintian-brush.git";
        let fixed = fix_vcs_git_user_url(url).unwrap();
        assert_eq!(
            fixed,
            "https://anonscm.debian.org/git/users/jelmer/lintian-brush.git"
        );
    }

    #[test]
    fn test_fix_vcs_git_user_url_with_subdir() {
        let url = "git://git.debian.org/~user/path/to/repo.git";
        let fixed = fix_vcs_git_user_url(url).unwrap();
        assert_eq!(
            fixed,
            "https://anonscm.debian.org/git/users/user/path/to/repo.git"
        );
    }

    #[test]
    fn test_fix_vcs_git_user_url_already_https() {
        let url = "https://anonscm.debian.org/git/users/user/repo.git";
        let fixed = fix_vcs_git_user_url(url);
        assert!(fixed.is_none());
    }

    #[test]
    fn test_fix_vcs_git_user_url_non_user_repo() {
        let url = "git://git.debian.org/collab-maint/project.git";
        let fixed = fix_vcs_git_user_url(url);
        assert!(fixed.is_none());
    }

    #[test]
    fn test_fix_vcs_git_user_url_different_host() {
        let url = "git://github.com/~user/repo.git";
        let fixed = fix_vcs_git_user_url(url);
        assert!(fixed.is_none());
    }

    #[test]
    fn test_run_fixes_control_file() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Vcs-Git: git://git.debian.org/~user/test-package.git

Package: test-package
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(base_path).unwrap();
        assert!(result
            .description
            .contains("Use valid URI for personal Debian Git repository"));

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content
            .contains("Vcs-Git: https://anonscm.debian.org/git/users/user/test-package.git"));
        assert!(!updated_content.contains("git://git.debian.org"));
    }

    #[test]
    fn test_run_no_changes_when_already_valid() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Vcs-Git: https://anonscm.debian.org/git/users/user/test-package.git

Package: test-package
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_run_no_changes_when_no_vcs_git() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package

Package: test-package
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
