use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType, Visibility};
use debian_workspace::Workspace;
use regex::Regex;
use std::path::PathBuf;

/// Fix the VCS Git URL from git://(git|anonscm).debian.org/~user/repo.git
/// to https://anonscm.debian.org/git/users/user/repo.git
pub fn fix_vcs_git_user_url(url: &str) -> Option<String> {
    let re = Regex::new(r"^git://(?:git|anonscm)\.debian\.org/~(.+)$").ok()?;
    let captures = re.captures(url)?;
    let user_and_path = captures.get(1)?.as_str();
    Some(format!(
        "https://anonscm.debian.org/git/users/{}",
        user_and_path
    ))
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
    let Some(vcs_git) = source.get("Vcs-Git") else {
        return Ok(Vec::new());
    };
    let Some(new_url) = fix_vcs_git_user_url(&vcs_git) else {
        return Ok(Vec::new());
    };

    let issue = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        visibility: Some(Visibility::Warning),
        tag: Some("vcs-git-uses-invalid-user-uri".to_string()),
        info: Some(vcs_git),
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Vcs-Git URI for personal Debian Git repository is invalid.",
        "Use valid URI for personal Debian Git repository.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Vcs-Git".into(),
            value: new_url,
        })],
    )])
}

declare_detector! {
    name: "vcs-git-uses-invalid-user-uri",
    tags: ["vcs-git-uses-invalid-user-uri"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Vcs-Git",
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
                Some("test-package".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

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
        assert!(fix_vcs_git_user_url(url).is_none());
    }

    #[test]
    fn test_fix_vcs_git_user_url_non_user_repo() {
        let url = "git://git.debian.org/collab-maint/project.git";
        assert!(fix_vcs_git_user_url(url).is_none());
    }

    #[test]
    fn test_fix_vcs_git_user_url_different_host() {
        let url = "git://github.com/~user/repo.git";
        assert!(fix_vcs_git_user_url(url).is_none());
    }

    #[test]
    fn test_run_fixes_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nVcs-Git: git://git.debian.org/~user/test-package.git\n\nPackage: test-package\nDescription: Test package\n Test description\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Use valid URI for personal Debian Git repository."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\nVcs-Git: https://anonscm.debian.org/git/users/user/test-package.git\n\nPackage: test-package\nDescription: Test package\n Test description\n",
        );
    }

    #[test]
    fn test_run_no_changes_when_already_valid() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nVcs-Git: https://anonscm.debian.org/git/users/user/test-package.git\n\nPackage: test-package\nDescription: Test package\n Test description\n",
        )
        .unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_run_no_changes_when_no_vcs_git() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\n\nPackage: test-package\nDescription: Test package\n Test description\n",
        )
        .unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }
}
