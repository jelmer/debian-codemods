use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

pub fn run(base_path: &Path, package: &str) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut made_changes = false;
    let mut original_maintainer = String::new();

    // Only process the source paragraph
    if let Some(mut source) = editor.source() {
        if let Some(maintainer) = source.maintainer() {
            let maintainer_str = maintainer.to_string();
            if maintainer_str.trim_end().ends_with(',') {
                // Store the original value for the issue info
                original_maintainer = maintainer_str.clone();
                // Remove the trailing comma
                let new_value = maintainer_str.trim_end().trim_end_matches(',').trim_end();
                source.set_maintainer(new_value);
                made_changes = true;
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    let mut issue = LintianIssue::source_with_info(
        "trailing-comma-in-maintainer-field",
        vec![original_maintainer],
    );
    issue.package = Some(package.to_string());

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    editor.commit()?;

    Ok(
        FixerResult::builder("Remove trailing comma from Maintainer field.")
            .certainty(crate::Certainty::Certain)
            .fixed_issue(issue)
            .build(),
    )
}

declare_fixer! {
    name: "trailing-comma-in-maintainer-field",
    tags: ["trailing-comma-in-maintainer-field"],
    apply: |basedir, package, _version, _preferences| {
        run(basedir, package)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_remove_trailing_comma() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: John Doe <john@example.com>,

Package: test-package
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Maintainer: John Doe <john@example.com>\n"));
        assert!(!updated_content.contains("Maintainer: John Doe <john@example.com>,"));
    }

    #[test]
    fn test_no_trailing_comma() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: John Doe <john@example.com>

Package: test-package
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_trailing_comma_with_whitespace() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Jane Smith <jane@example.com> ,

Package: test-package
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Maintainer: Jane Smith <jane@example.com>\n"));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
