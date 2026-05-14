use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

fn extract_email_address(address_str: &str) -> String {
    if let Some(start) = address_str.find('<') {
        if let Some(end) = address_str.find('>') {
            if end > start {
                return address_str[start + 1..end].to_string();
            }
        }
    }
    address_str.trim().to_string()
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    let Some(maintainer) = source.get("Maintainer") else {
        return Ok(Vec::new());
    };
    if extract_email_address(&maintainer) != "packages@qa.debian.org" {
        return Ok(Vec::new());
    }
    if !source.as_deb822().contains_key("Uploaders") {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "uploaders-in-orphan",
        Visibility::Error,
        vec!["[debian/changelog:1]".to_string()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Orphaned package has Uploaders.",
        "Remove uploaders from orphaned package.",
        vec![Action::Deb822(Deb822Action::RemoveField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Uploaders".into(),
        })],
    )])
}

declare_detector! {
    name: "orphaned-package-should-not-have-uploaders",
    tags: ["uploaders-in-orphan"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Maintainer",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Uploaders",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test-package", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_uploaders_from_orphaned_package() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nMaintainer: Debian QA Team <packages@qa.debian.org>\nUploaders: Somebody <somebody@example.com>\n",
        )
        .unwrap();

        run_apply(temp_dir.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\nMaintainer: Debian QA Team <packages@qa.debian.org>\n",
        );
    }

    #[test]
    fn test_no_change_for_non_orphaned_package() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: test-package\nMaintainer: Regular Maintainer <maintainer@example.com>\nUploaders: Somebody <somebody@example.com>\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_no_change_orphaned_package_without_uploaders() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nMaintainer: Debian QA Team <packages@qa.debian.org>\n",
        )
        .unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
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
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
