use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};

const MESSAGE: &str = "Replace XC-Package-Type with Package-Type.";

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_path = base_path.join(&control_rel);
    if !control_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&control_path)?;
    let control: Control = content.parse().map_err(|_| FixerError::NoChanges)?;
    let mut diagnostics = Vec::new();

    if let Some(source) = control.source() {
        let paragraph = source.as_deb822();
        if let Some(entry) = paragraph.get_entry("XC-Package-Type") {
            let line_number = entry.line() + 1;
            let issue = LintianIssue::source_with_info(
                "adopted-extended-field",
                vec![format!(
                    "(in section for source) XC-Package-Type [debian/control:{}]",
                    line_number
                )],
            );
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    MESSAGE,
                    vec![Action::Deb822(Deb822Action::RenameField {
                        file: control_rel.clone(),
                        paragraph: ParagraphSelector::Source,
                        from: "XC-Package-Type".into(),
                        to: "Package-Type".into(),
                    })],
                )
                .with_certainty(Certainty::Certain),
            );
        }
    }

    for binary in control.binaries() {
        let Some(package_name) = binary.name() else {
            continue;
        };
        let paragraph = binary.as_deb822();
        let Some(entry) = paragraph.get_entry("XC-Package-Type") else {
            continue;
        };
        let line_number = entry.line() + 1;
        let issue = LintianIssue::binary_with_info(
            &package_name,
            "adopted-extended-field",
            vec![format!(
                "(in section for {}) XC-Package-Type [debian/control:{}]",
                package_name, line_number
            )],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                MESSAGE,
                vec![Action::Deb822(Deb822Action::RenameField {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: package_name,
                    },
                    from: "XC-Package-Type".into(),
                    to: "Package-Type".into(),
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_fixer! {
    name: "xc-package-type-in-debian-control",
    tags: ["adopted-extended-field"],
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
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_xc_package_type_in_source() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nXC-Package-Type: deb\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, MESSAGE);
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nPackage-Type: deb\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_xc_package_type_in_binary() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\n\nPackage: test\nXC-Package-Type: udeb\nDescription: Test\n Test package\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\n\nPackage: test\nPackage-Type: udeb\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_no_xc_package_type() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original =
            "Source: test\nPackage-Type: deb\n\nPackage: test\nDescription: Test\n Test package\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_multiple_binaries() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\n\nPackage: test1\nXC-Package-Type: deb\nDescription: Test 1\n Test package 1\n\nPackage: test2\nXC-Package-Type: udeb\nDescription: Test 2\n Test package 2\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(result.fixed_lintian_issues.len(), 2);

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\n\nPackage: test1\nPackage-Type: deb\nDescription: Test 1\n Test package 1\n\nPackage: test2\nPackage-Type: udeb\nDescription: Test 2\n Test package 2\n",
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
