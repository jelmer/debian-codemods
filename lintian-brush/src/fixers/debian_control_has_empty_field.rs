use crate::{declare_fixer, FixerError, FixerResult};
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
    let mut made_changes = false;

    // Check source paragraph for empty fields
    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();
        let keys_to_remove: Vec<String> = paragraph
            .keys()
            .filter(|key| {
                if let Some(value) = paragraph.get(key) {
                    value.trim().is_empty()
                } else {
                    false
                }
            })
            .map(|key| key.to_string())
            .collect();

        for key in keys_to_remove {
            paragraph.remove(&key);
            removed_fields.push(key);
            made_changes = true;
        }
    }

    // Check binary paragraphs for empty fields
    for mut binary in editor.binaries() {
        let paragraph = binary.as_mut_deb822();
        let package_name = paragraph
            .get("Package")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let keys_to_remove: Vec<String> = paragraph
            .keys()
            .filter(|key| {
                if let Some(value) = paragraph.get(key) {
                    value.trim().is_empty()
                } else {
                    false
                }
            })
            .map(|key| key.to_string())
            .collect();

        if !keys_to_remove.is_empty() {
            packages_affected.push(package_name);
        }

        for key in keys_to_remove {
            paragraph.remove(&key);
            removed_fields.push(key);
            made_changes = true;
        }
    }

    if !made_changes {
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
        .fixed_tags(vec!["debian-control-has-empty-field"])
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
