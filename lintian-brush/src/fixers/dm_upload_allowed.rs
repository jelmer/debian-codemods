use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let mut editor = TemplatedControlEditor::open(&control_path)?;
    let mut made_changes = false;

    // Only process the source paragraph (DM-Upload-Allowed only appears there)
    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        // Check if DM-Upload-Allowed field exists and remove it
        if paragraph.contains_key("DM-Upload-Allowed") {
            paragraph.remove("DM-Upload-Allowed");
            made_changes = true;
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    Ok(FixerResult::builder(
        "Remove malformed and unnecessary DM-Upload-Allowed field in debian/control.",
    )
    .fixed_tags(vec!["malformed-dm-upload-allowed"])
    .build())
}

declare_fixer! {
    name: "dm-upload-allowed",
    tags: ["malformed-dm-upload-allowed", "dm-upload-allowed-is-obsolete"],
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
    fn test_dm_upload_allowed_removed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: lintian-brush\nDM-Upload-Allowed: yes\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove malformed and unnecessary DM-Upload-Allowed field in debian/control."
        );

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(!content.contains("DM-Upload-Allowed"));
        assert!(content.contains("Source: lintian-brush"));
        assert!(content.contains("Package: lintian-brush"));
    }

    #[test]
    fn test_no_dm_upload_allowed_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nMaintainer: Test <test@example.com>\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

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

    #[test]
    fn test_multiple_fields_dm_upload_allowed_removed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nMaintainer: Test <test@example.com>\nDM-Upload-Allowed: yes\nHomepage: https://example.com\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove malformed and unnecessary DM-Upload-Allowed field in debian/control."
        );

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(!content.contains("DM-Upload-Allowed"));
        assert!(content.contains("Maintainer: Test <test@example.com>"));
        assert!(content.contains("Homepage: https://example.com"));
    }
}
