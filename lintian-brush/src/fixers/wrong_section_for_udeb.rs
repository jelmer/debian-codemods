use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const CERTAINTY: Certainty = Certainty::Certain;
const EXPECTED_SECTION: &str = "debian-installer";

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let default_section = control.source().as_ref().and_then(|s| s.get("Section"));

    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        if binary.get("Package-Type").as_deref().map(str::trim) != Some("udeb") {
            continue;
        }

        let Some(package_name) = binary.name() else {
            continue;
        };

        let current_section = binary
            .get("Section")
            .or_else(|| default_section.clone())
            .unwrap_or_default();

        if current_section == EXPECTED_SECTION {
            continue;
        }

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "wrong-section-for-udeb",
            Visibility::Warning,
            vec![current_section.clone()],
        );

        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                format!(
                    "Set Section to {} for udeb package {}.",
                    EXPECTED_SECTION, package_name
                ),
                format!(
                    "Set Section to {} for udeb package {}.",
                    EXPECTED_SECTION, package_name
                ),
                vec![Action::Deb822(Deb822Action::SetField {
                    file: PathBuf::from("debian/control"),
                    paragraph: ParagraphSelector::Binary {
                        package: package_name,
                    },
                    field: "Section".into(),
                    value: EXPECTED_SECTION.to_string(),
                })],
            )
            .with_certainty(CERTAINTY),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "wrong-section-for-udeb",
    tags: ["wrong-section-for-udeb"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Section",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Section",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package-Type",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply_with(
        base: &Path,
        prefs: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        adapter.apply(&ws, prefs)
    }

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        run_apply_with(base, &FixerPreferences::default())
    }

    fn write_control(dir: &Path, body: &str) {
        let debian_dir = dir.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        fs::write(debian_dir.join("control"), body).unwrap();
    }

    #[test]
    fn test_no_control() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_fix_udeb_section() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nSection: admin\nMaintainer: Test <test@example.com>\n\n\
             Package: foo-udeb\nPackage-Type: udeb\nSection: admin\n\
             Architecture: all\nDescription: foo udeb\n udeb for foo\n",
        );

        let result = run_apply(base).unwrap();
        assert!(result.description.contains("debian-installer"));

        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nSection: admin\nMaintainer: Test <test@example.com>\n\n\
             Package: foo-udeb\nPackage-Type: udeb\nSection: debian-installer\n\
             Architecture: all\nDescription: foo udeb\n udeb for foo\n",
        );
    }

    #[test]
    fn test_fix_udeb_inherits_default_section() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nSection: admin\nMaintainer: Test <test@example.com>\n\n\
             Package: foo-udeb\nPackage-Type: udeb\nArchitecture: all\n\
             Description: foo udeb\n udeb for foo\n",
        );

        run_apply(base).unwrap();

        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nSection: admin\nMaintainer: Test <test@example.com>\n\n\
             Package: foo-udeb\nPackage-Type: udeb\nArchitecture: all\n\
             Section: debian-installer\nDescription: foo udeb\n udeb for foo\n",
        );
    }

    #[test]
    fn test_no_change_already_debian_installer() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let body = "Source: foo\nSection: admin\nMaintainer: Test <test@example.com>\n\n\
                    Package: foo-udeb\nPackage-Type: udeb\nSection: debian-installer\n\
                    Architecture: all\nDescription: foo udeb\n udeb for foo\n";
        write_control(base, body);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            body
        );
    }

    #[test]
    fn test_skip_non_udeb() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let body = "Source: foo\nSection: admin\nMaintainer: Test <test@example.com>\n\n\
                    Package: foo\nSection: admin\nArchitecture: all\n\
                    Description: foo\n foo package\n";
        write_control(base, body);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            body
        );
    }

    #[test]
    fn test_fix_multiple_udebs() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nMaintainer: Test <test@example.com>\n\n\
             Package: foo-udeb\nPackage-Type: udeb\nSection: admin\n\
             Architecture: all\nDescription: foo udeb\n udeb for foo\n\n\
             Package: bar-udeb\nPackage-Type: udeb\nSection: net\n\
             Architecture: all\nDescription: bar udeb\n udeb for bar\n",
        );

        run_apply(base).unwrap();

        let out = fs::read_to_string(base.join("debian/control")).unwrap();
        assert_eq!(
            out,
            "Source: foo\nMaintainer: Test <test@example.com>\n\n\
             Package: foo-udeb\nPackage-Type: udeb\nSection: debian-installer\n\
             Architecture: all\nDescription: foo udeb\n udeb for foo\n\n\
             Package: bar-udeb\nPackage-Type: udeb\nSection: debian-installer\n\
             Architecture: all\nDescription: bar udeb\n udeb for bar\n",
        );
    }
}
