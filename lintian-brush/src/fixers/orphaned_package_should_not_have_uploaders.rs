use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

fn extract_email_address(address_str: &str) -> String {
    // Simple email extraction - look for text between < and >
    if let Some(start) = address_str.find('<') {
        if let Some(end) = address_str.find('>') {
            if end > start {
                return address_str[start + 1..end].to_string();
            }
        }
    }

    // If no angle brackets found, assume the whole string is the email
    // after trimming whitespace
    address_str.trim().to_string()
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        // Check if maintainer is packages@qa.debian.org and uploaders field exists
        if let Some(maintainer) = paragraph.get("Maintainer") {
            let email = extract_email_address(&maintainer);

            if email == "packages@qa.debian.org" && paragraph.contains_key("Uploaders") {
                let issue = LintianIssue::source_with_info(
                    "uploaders-in-orphan",
                    vec!["[debian/changelog:1]".to_string()],
                );

                if !issue.should_fix(base_path) {
                    return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
                }

                paragraph.remove("Uploaders");
                editor.commit()?;

                return Ok(
                    FixerResult::builder("Remove uploaders from orphaned package")
                        .fixed_issue(issue)
                        .build(),
                );
            }
        }
    }

    Err(FixerError::NoChanges)
}

declare_fixer! {
    name: "orphaned-package-should-not-have-uploaders",
    tags: ["uploaders-in-orphan"],
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
    fn test_remove_uploaders_from_orphaned_package() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Debian QA Team <packages@qa.debian.org>
Uploaders: Somebody <somebody@example.com>
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        // Check that Uploaders field was removed
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("Uploaders:"));
        assert!(updated_content.contains("Maintainer: Debian QA Team <packages@qa.debian.org>"));
    }

    #[test]
    fn test_no_change_for_non_orphaned_package() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Regular Maintainer <maintainer@example.com>
Uploaders: Somebody <somebody@example.com>
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        // Apply the fixer
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
    fn test_no_change_orphaned_package_without_uploaders() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Debian QA Team <packages@qa.debian.org>
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        // Apply the fixer
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
    fn test_email_extraction() {
        assert_eq!(
            extract_email_address("Debian QA Team <packages@qa.debian.org>"),
            "packages@qa.debian.org"
        );
        assert_eq!(
            extract_email_address("packages@qa.debian.org"),
            "packages@qa.debian.org"
        );
        assert_eq!(
            extract_email_address("  packages@qa.debian.org  "),
            "packages@qa.debian.org"
        );
        assert_eq!(
            extract_email_address("Name <email@example.com>"),
            "email@example.com"
        );
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();

        // Apply the fixer
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
