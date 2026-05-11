use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &crate::FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let control_rel = PathBuf::from("debian/control");
    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        let Some(value) = binary.as_deb822().get("Essential") else {
            continue;
        };
        if value.trim() != "no" {
            continue;
        }
        let Some(package) = binary.name() else {
            continue;
        };

        let issue = LintianIssue::binary_with_info(
            &package,
            "essential-no-not-needed",
            Visibility::Warning,
            vec![],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                format!("Package {} has redundant Essential: no.", package),
                format!("Remove redundant Essential: no from package {}.", package),
                vec![Action::Deb822(Deb822Action::RemoveField {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary { package },
                    field: "Essential".into(),
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "essential-no-not-needed",
    tags: ["essential-no-not-needed"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Essential",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FixerPreferences, Version};
    use debian_workspace::{DetectorAdapter, FsWorkspace};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    fn run_detect(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let ws = FsWorkspace::new(base, "test", version);
        detect(&ws, &FixerPreferences::default())
    }

    #[test]
    fn test_removes_essential_no() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: pkg\n\nPackage: pkg\nArchitecture: any\nEssential: no\nDescription: test\n .\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: pkg\n\nPackage: pkg\nArchitecture: any\nDescription: test\n .\n",
        );
    }

    #[test]
    fn test_essential_yes_kept() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: pkg\n\nPackage: pkg\nArchitecture: any\nEssential: yes\nDescription: test\n .\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_essential_field() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: pkg\n\nPackage: pkg\nArchitecture: any\nDescription: test\n .\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_detect_returns_diagnostics() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: pkg\n\nPackage: a\nArchitecture: any\nEssential: no\nDescription: test\n .\n\nPackage: b\nArchitecture: any\nDescription: test\n .\n",
        )
        .unwrap();

        let diagnostics = run_detect(tmp.path()).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].issue.as_ref().unwrap().package.as_deref(),
            Some("a"),
        );
    }
}
