use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Check source paragraph
    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();
        if paragraph.get("Priority").as_deref() == Some("extra") {
            let issue = LintianIssue::source_with_info(
                "priority-extra-is-replaced-by-priority-optional",
                vec![],
            );

            if !issue.should_fix(base_path) {
                overridden_issues.push(issue);
            } else {
                paragraph.set("Priority", "optional");
                fixed_issues.push(issue);
            }
        }
    }

    // Check binary paragraphs
    for mut binary in editor.binaries() {
        let Some(package_name) = binary.name() else {
            continue;
        };
        let paragraph = binary.as_mut_deb822();
        if paragraph.get("Priority").as_deref() == Some("extra") {
            let issue = LintianIssue::binary_with_info(
                &package_name,
                "priority-extra-is-replaced-by-priority-optional",
                vec![],
            );

            if !issue.should_fix(base_path) {
                overridden_issues.push(issue);
            } else {
                paragraph.set("Priority", "optional");
                fixed_issues.push(issue);
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    Ok(
        FixerResult::builder("Change priority extra to priority optional.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .certainty(crate::Certainty::Certain)
            .build(),
    )
}

declare_fixer! {
    name: "priority-extra-is-replaced-by-priority-optional",
    tags: ["priority-extra-is-replaced-by-priority-optional"],
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
    fn test_change_priority_extra_to_optional() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = "\
Source: test-package
Priority: extra

Package: test-package
Priority: extra
Description: Test package
 This is a test package.
";

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Change priority extra to priority optional."
        );
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that the file was updated
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Priority: optional"));
        assert!(!updated_content.contains("Priority: extra"));
    }

    #[test]
    fn test_source_only_priority_extra() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = "\
Source: test-package
Priority: extra

Package: test-package
Description: Test package
 This is a test package.
";

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that only source priority was changed
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Priority: optional"));
        assert!(!updated_content.contains("Priority: extra"));
    }

    #[test]
    fn test_binary_only_priority_extra() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = "\
Source: test-package

Package: test-package
Priority: extra
Description: Test package
 This is a test package.
";

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that only binary priority was changed
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Priority: optional"));
        assert!(!updated_content.contains("Priority: extra"));
    }

    #[test]
    fn test_no_priority_extra() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = "\
Source: test-package
Priority: optional

Package: test-package
Priority: optional
Description: Test package
 This is a test package.
";

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Check that nothing was changed
        let content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(content, control_content);
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
