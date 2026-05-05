use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType};
use std::path::PathBuf;

const FIXABLE_HOSTS: &[&str] = &[
    "gitlab.com",
    "github.com",
    "salsa.debian.org",
    "gitorious.org",
];

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let Some(vcs_git) = source.get("Vcs-Git") else {
        return Ok(Vec::new());
    };

    if !vcs_git.contains(':') {
        return Ok(Vec::new());
    }
    let parts: Vec<&str> = vcs_git.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Ok(Vec::new());
    }
    let mut netloc = parts[0];
    let path = parts[1];
    if let Some(stripped) = netloc.strip_prefix("git@") {
        netloc = stripped;
    }
    if !FIXABLE_HOSTS.contains(&netloc) {
        return Ok(Vec::new());
    }

    let new_url = format!("https://{}/{}", netloc, path);

    let issue = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        tag: Some("vcs-field-uses-not-recommended-uri-format".to_string()),
        info: Some(format!("vcs-git {}", vcs_git)),
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Use recommended URI format in Vcs header.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Vcs-Git".into(),
            value: new_url,
        })],
    )])
}

declare_detector! {
    name: "vcs-field-uses-not-recommended-uri-format",
    tags: ["vcs-field-uses-not-recommended-uri-format"],
    // Must improve URI format after securing them and before adding browser field
    after: ["vcs-field-uses-insecure-uri"],
    before: ["missing-vcs-browser-field"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::workspace::DetectorAdapter;
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
    fn test_converts_git_ssh_url() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nVcs-Git: git@github.com:user/repo.git\n\nPackage: test\nDescription: test\n",
        )
        .unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Use recommended URI format in Vcs header."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nVcs-Git: https://github.com/user/repo.git\n\nPackage: test\nDescription: test\n",
        );
    }

    #[test]
    fn test_no_change_when_already_https() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: test\nVcs-Git: https://github.com/user/repo.git\n\nPackage: test\nDescription: test\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_no_change_when_no_vcs_git() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\n\nPackage: test\nDescription: test\n",
        )
        .unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_change_when_no_control() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
