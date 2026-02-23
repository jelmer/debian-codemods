use crate::{declare_fixer, FixerError, FixerResult, LintianIssue, PackageType};
use debian_analyzer::abstract_control::AbstractSource;
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

const FIXABLE_HOSTS: &[&str] = &[
    "gitlab.com",
    "github.com",
    "salsa.debian.org",
    "gitorious.org",
];

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

    // Check if the URL contains a colon (SSH-style format)
    if !vcs_git.contains(':') {
        return Err(FixerError::NoChanges);
    }

    // Split on the first colon
    let parts: Vec<&str> = vcs_git.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(FixerError::NoChanges);
    }

    let mut netloc = parts[0];
    let path = parts[1];

    // Strip git@ prefix if present
    if let Some(stripped) = netloc.strip_prefix("git@") {
        netloc = stripped;
    }

    // Check if this is a fixable host
    if !FIXABLE_HOSTS.contains(&netloc) {
        return Err(FixerError::NoChanges);
    }

    let old_url = vcs_git.clone();
    let new_url = format!("https://{}/{}", netloc, path);

    // Create the issue before modifying
    let issue = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        tag: Some("vcs-field-uses-not-recommended-uri-format".to_string()),
        info: Some(format!("vcs-git {}", old_url)),
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
        FixerResult::builder("Use recommended URI format in Vcs header.")
            .fixed_issue(issue)
            .build(),
    )
}

declare_fixer! {
    name: "vcs-field-uses-not-recommended-uri-format",
    tags: ["vcs-field-uses-not-recommended-uri-format"],
    // Must improve URI format after securing them and before adding browser field
    after: ["vcs-field-uses-insecure-uri"],
    before: ["missing-vcs-browser-field"],
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
    fn test_converts_git_ssh_url() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: test\nVcs-Git: git@github.com:user/repo.git\n\nPackage: test\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Vcs-Git: https://github.com/user/repo.git"));
        assert!(!updated_content.contains("git@github.com"));
    }

    #[test]
    fn test_no_change_when_already_https() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: test\nVcs-Git: https://github.com/user/repo.git\n\nPackage: test\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_vcs_git() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: test\n\nPackage: test\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_control() {
        let temp_dir = TempDir::new().unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
