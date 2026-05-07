use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let Some(value) = source.get("DM-Upload-Allowed") else {
        return Ok(Vec::new());
    };

    let issue = LintianIssue::source_with_info("malformed-dm-upload-allowed", vec![value]);
    Ok(vec![Diagnostic::with_actions(
        issue,
        "DM-Upload-Allowed field in debian/control is obsolete.",
        "Remove malformed and unnecessary DM-Upload-Allowed field in debian/control.",
        vec![Action::Deb822(Deb822Action::RemoveField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "DM-Upload-Allowed".into(),
        })],
    )])
}

declare_detector! {
    name: "dm-upload-allowed",
    tags: ["malformed-dm-upload-allowed", "dm-upload-allowed-is-obsolete"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "DM-Upload-Allowed",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_dm_upload_allowed_removed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: lintian-brush\nDM-Upload-Allowed: yes\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove malformed and unnecessary DM-Upload-Allowed field in debian/control."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        );
    }

    #[test]
    fn test_no_dm_upload_allowed_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nMaintainer: Test <test@example.com>\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_multiple_fields_dm_upload_allowed_removed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nMaintainer: Test <test@example.com>\nDM-Upload-Allowed: yes\nHomepage: https://example.com\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove malformed and unnecessary DM-Upload-Allowed field in debian/control."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nMaintainer: Test <test@example.com>\nHomepage: https://example.com\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }
}
