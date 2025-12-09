use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        if let Some(testsuite) = paragraph.get("Testsuite") {
            if testsuite == "autopkgtest" {
                let line_number = paragraph
                    .get_entry("Testsuite")
                    .map(|e| e.line() + 1)
                    .unwrap_or(1);

                let issue = LintianIssue::source_with_info(
                    "unnecessary-testsuite-autopkgtest-field",
                    vec![format!("[debian/control:{}]", line_number)],
                );

                if !issue.should_fix(base_path) {
                    return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
                }

                paragraph.remove("Testsuite");
                editor.commit()?;

                return Ok(FixerResult::builder(
                    "Remove unnecessary 'Testsuite: autopkgtest' header",
                )
                .fixed_issue(issue)
                .build());
            }
        }
    }

    Err(FixerError::NoChanges)
}

declare_fixer! {
    name: "unnecessary-testsuite-autopkgtest-field",
    tags: ["unnecessary-testsuite-autopkgtest-field"],
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
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_remove_autopkgtest_testsuite() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: lintian-brush\nTestsuite: autopkgtest\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Remove unnecessary 'Testsuite: autopkgtest' header"
        );

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("Testsuite:"));
        assert!(updated_content.contains("Source: lintian-brush"));
        assert!(updated_content.contains("Package: lintian-brush"));
    }

    #[test]
    fn test_no_testsuite_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: lintian-brush\n\nPackage: lintian-brush\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_different_testsuite_value() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: lintian-brush\nTestsuite: other-value\n\nPackage: lintian-brush\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Verify the field was not removed
        let content = fs::read_to_string(&control_path).unwrap();
        assert!(content.contains("Testsuite: other-value"));
    }
}
