use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut made_changes = false;
    let mut fixed_issue = None;

    if let Some(mut source) = editor.source() {
        let source_para = source.as_mut_deb822();

        // Check if Vcs-Browser is already present
        if source_para.get("Vcs-Browser").is_some() {
            return Err(FixerError::NoChanges);
        }

        // Check if Vcs-Git is present
        if let Some(vcs_git) = source_para.get("Vcs-Git") {
            // Determine the browser URL from the Git URL
            let browser_url = debian_analyzer::vcs::determine_browser_url(
                "git",
                &vcs_git,
                preferences.net_access,
            );

            if let Some(browser_url) = browser_url {
                let issue = LintianIssue::source_with_info(
                    "missing-vcs-browser-field",
                    vec![format!("Vcs-Git {}", vcs_git)],
                );

                if !issue.should_fix(base_path) {
                    return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
                }

                source_para.insert("Vcs-Browser", browser_url.as_ref());
                made_changes = true;
                fixed_issue = Some(issue);
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    // fixed_issue is guaranteed to be Some if made_changes is true
    let issue = fixed_issue.expect("fixed_issue should be Some when made_changes is true");

    Ok(
        FixerResult::builder("debian/control: Add Vcs-Browser field")
            .fixed_issue(issue)
            .build(),
    )
}

declare_fixer! {
    name: "missing-vcs-browser-field",
    tags: ["missing-vcs-browser-field"],
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
    fn test_add_vcs_browser_for_github() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Vcs-Git: git://github.com/user/repo

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false), // Disable network for tests
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.description, "debian/control: Add Vcs-Browser field");

        // Check that the file was updated
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Vcs-Browser:"));
        assert!(updated_content.contains("https://github.com/user/repo"));
    }

    #[test]
    fn test_no_change_when_vcs_browser_exists() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Vcs-Git: git://github.com/user/repo
Vcs-Browser: https://github.com/user/repo

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_vcs_git() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_add_vcs_browser_for_salsa() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Vcs-Git: https://salsa.debian.org/debian/test-package.git

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.description, "debian/control: Add Vcs-Browser field");

        // Check that the file was updated
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Vcs-Browser:"));
        assert!(updated_content.contains("https://salsa.debian.org/debian/test-package"));
    }
}
