use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};

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

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_path = base_path.join(&control_rel);
    if !control_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&control_path)?;
    let control: Control = content.parse().map_err(|_| FixerError::NoChanges)?;
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let paragraph = source.as_deb822();

    let Some(maintainer) = paragraph.get("Maintainer") else {
        return Ok(Vec::new());
    };
    if extract_email_address(&maintainer) != "packages@qa.debian.org" {
        return Ok(Vec::new());
    }
    if !paragraph.contains_key("Uploaders") {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "uploaders-in-orphan",
        vec!["[debian/changelog:1]".to_string()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Remove uploaders from orphaned package.",
        vec![Action::Deb822(Deb822Action::RemoveField {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Uploaders".into(),
        })],
    )])
}

declare_fixer! {
    name: "orphaned-package-should-not-have-uploaders",
    tags: ["uploaders-in-orphan"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test-package", &version, &FixerPreferences::default())
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
