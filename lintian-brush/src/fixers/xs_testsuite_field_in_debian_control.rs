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

    // Only process the source paragraph (XS-Testsuite only appears there)
    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        // Check if XS-Testsuite field exists
        if let Some(entry) = paragraph.get_entry("XS-Testsuite") {
            let line_number = entry.line() + 1;
            let issue = LintianIssue::source_with_info(
                "adopted-extended-field",
                vec![format!(
                    "(in section for source) XS-Testsuite [debian/control:{}]",
                    line_number
                )],
            );

            if !issue.should_fix(base_path) {
                overridden_issues.push(issue);
            } else {
                let value = paragraph.get("XS-Testsuite").unwrap_or_default();
                if value.trim() == "autopkgtest" {
                    // Remove XS-Testsuite: autopkgtest entirely (it's the default now)
                    paragraph.remove("XS-Testsuite");
                } else {
                    // Rename XS-Testsuite to Testsuite for other values
                    paragraph.rename("XS-Testsuite", "Testsuite");
                }
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
        FixerResult::builder("Remove unnecessary XS-Testsuite field in debian/control.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .certainty(crate::Certainty::Certain)
            .build(),
    )
}

declare_fixer! {
    name: "xs-testsuite-field-in-debian-control",
    tags: ["adopted-extended-field"],
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
    fn test_xs_testsuite_autopkgtest_removed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nXS-Testsuite: autopkgtest\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove unnecessary XS-Testsuite field in debian/control."
        );
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(!content.contains("XS-Testsuite"));
        assert!(!content.contains("Testsuite"));
    }

    #[test]
    fn test_xs_testsuite_renamed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nXS-Testsuite: custom-test\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(!content.contains("XS-Testsuite"));
        assert!(content.contains("Testsuite: custom-test"));
    }

    #[test]
    fn test_no_xs_testsuite() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nTestsuite: autopkgtest\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
