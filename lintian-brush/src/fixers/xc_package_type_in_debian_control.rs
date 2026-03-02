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

    // Check and replace XC-Package-Type in source paragraph
    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();
        if let Some(entry) = paragraph.get_entry("XC-Package-Type") {
            let line_number = entry.line() + 1;
            let issue = LintianIssue::source_with_info(
                "adopted-extended-field",
                vec![format!(
                    "(in section for source) XC-Package-Type [debian/control:{}]",
                    line_number
                )],
            );

            if !issue.should_fix(base_path) {
                overridden_issues.push(issue);
            } else {
                paragraph.rename("XC-Package-Type", "Package-Type");
                fixed_issues.push(issue);
            }
        }
    }

    // Check and replace XC-Package-Type in binary paragraphs
    for mut binary in editor.binaries() {
        let Some(package_name) = binary.name() else {
            continue;
        };
        let paragraph = binary.as_mut_deb822();
        if let Some(entry) = paragraph.get_entry("XC-Package-Type") {
            let line_number = entry.line() + 1;
            let issue = LintianIssue::binary_with_info(
                &package_name,
                "adopted-extended-field",
                vec![format!(
                    "(in section for {}) XC-Package-Type [debian/control:{}]",
                    package_name, line_number
                )],
            );

            if !issue.should_fix(base_path) {
                overridden_issues.push(issue);
            } else {
                paragraph.rename("XC-Package-Type", "Package-Type");
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
        FixerResult::builder("Replace XC-Package-Type with Package-Type.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .certainty(crate::Certainty::Certain)
            .build(),
    )
}

declare_fixer! {
    name: "xc-package-type-in-debian-control",
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
    fn test_xc_package_type_in_source() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nXC-Package-Type: deb\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Replace XC-Package-Type with Package-Type."
        );
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(content.contains("Package-Type: deb"));
        assert!(!content.contains("XC-Package-Type"));
    }

    #[test]
    fn test_xc_package_type_in_binary() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\n\nPackage: test\nXC-Package-Type: udeb\nDescription: Test\n Test package\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(content.contains("Package-Type: udeb"));
        assert!(!content.contains("XC-Package-Type"));
    }

    #[test]
    fn test_no_xc_package_type() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nPackage-Type: deb\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_binaries() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\n\nPackage: test1\nXC-Package-Type: deb\nDescription: Test 1\n Test package 1\n\nPackage: test2\nXC-Package-Type: udeb\nDescription: Test 2\n Test package 2\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        let content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(content.matches("Package-Type:").count(), 2);
        assert!(!content.contains("XC-Package-Type"));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
