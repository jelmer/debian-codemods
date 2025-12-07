use crate::{declare_fixer, FixerError, FixerResult, LintianIssue, PackageType};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut removed_fields = Vec::new();
    let mut packages_affected = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Check source paragraph for empty fields
    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        let mut keys_to_remove = Vec::new();
        for entry in paragraph.entries() {
            let value = entry.value();
            if value.trim().is_empty() {
                if let Some(key) = entry.key() {
                    let line_number = entry.line() + 1;
                    keys_to_remove.push((key.to_string(), line_number));
                }
            }
        }

        for (key, line_number) in keys_to_remove {
            let issue = LintianIssue::source_with_info(
                "debian-control-has-empty-field",
                vec![format!("(in source paragraph) {} [debian/control:{}]", key, line_number)],
            );

            if issue.should_fix(base_path) {
                paragraph.remove(&key);
                removed_fields.push(key);
                fixed_issues.push(issue);
            } else {
                overridden_issues.push(issue);
            }
        }
    }

    // Check binary paragraphs for empty fields
    for mut binary in editor.binaries() {
        let paragraph = binary.as_mut_deb822();
        let package_name = paragraph
            .get("Package")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let mut keys_to_remove = Vec::new();
        for entry in paragraph.entries() {
            let value = entry.value();
            if value.trim().is_empty() {
                if let Some(key) = entry.key() {
                    let line_number = entry.line() + 1;
                    keys_to_remove.push((key.to_string(), line_number));
                }
            }
        }

        for (key, line_number) in keys_to_remove {
            let issue = LintianIssue {
                package: Some(package_name.clone()),
                package_type: Some(PackageType::Binary),
                tag: Some("debian-control-has-empty-field".to_string()),
                info: Some(vec![format!("(in section for {}) {} [debian/control:{}]", package_name, key, line_number)]),
            };

            if issue.should_fix(base_path) {
                paragraph.remove(&key);
                removed_fields.push(key);
                packages_affected.push(package_name.clone());
                fixed_issues.push(issue);
            } else {
                overridden_issues.push(issue);
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Commit the changes
    editor.commit()?;

    // Create description message
    let field_text = if removed_fields.len() == 1 {
        "field"
    } else {
        "fields"
    };

    let package_text = if packages_affected.is_empty() {
        String::new()
    } else {
        format!(" in package {}", packages_affected.join(", "))
    };

    let description = format!(
        "debian/control: Remove empty control {} {}{}.",
        field_text,
        removed_fields.join(", "),
        package_text
    );

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "debian-control-has-empty-field",
    tags: ["debian-control-has-empty-field"],
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
    fn test_remove_empty_fields() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Depends:

Package: test-package
Description: Test package
 Description text
Provides:
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

        // Check that empty fields were removed
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("Depends:"));
        assert!(!updated_content.contains("Provides:"));
        assert!(updated_content.contains("Source: test-package"));
        assert!(updated_content.contains("Package: test-package"));
        assert!(updated_content.contains("Description: Test package"));
    }

    #[test]
    fn test_no_empty_fields() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Test Maintainer <test@example.com>

Package: test-package
Description: Test package
 Description text
Depends: libc6
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

    #[test]
    fn test_whitespace_only_fields() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Build-Depends:   

Package: test-package
Description: Test package
 Description text
Provides:  	
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

        // Check that whitespace-only fields were removed
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("Build-Depends:"));
        assert!(!updated_content.contains("Provides:"));
    }
}
