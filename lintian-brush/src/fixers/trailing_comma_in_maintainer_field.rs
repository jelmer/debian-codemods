use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path, package: &str) -> Result<Vec<Diagnostic>, FixerError> {
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
    let Some(maintainer) = source.as_deb822().get("Maintainer") else {
        return Ok(Vec::new());
    };
    if !maintainer.trim_end().ends_with(',') {
        return Ok(Vec::new());
    }

    let new_value = maintainer
        .trim_end()
        .trim_end_matches(',')
        .trim_end()
        .to_string();

    let mut issue = LintianIssue::source_with_info(
        "trailing-comma-in-maintainer-field",
        vec![maintainer.clone()],
    );
    issue.package = Some(package.to_string());

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Remove trailing comma from Maintainer field.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Maintainer".into(),
            value: new_value,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_fixer! {
    name: "trailing-comma-in-maintainer-field",
    tags: ["trailing-comma-in-maintainer-field"],
    diagnose: |basedir, package, _version, _preferences| {
        detect(basedir, package)
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
    fn test_remove_trailing_comma() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: John Doe <john@example.com>,

Package: test-package
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove trailing comma from Maintainer field."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\nMaintainer: John Doe <john@example.com>\n\nPackage: test-package\nDescription: Test package\n Test description\n",
        );
    }

    #[test]
    fn test_no_trailing_comma() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: John Doe <john@example.com>

Package: test-package
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_trailing_comma_with_whitespace() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Jane Smith <jane@example.com> ,

Package: test-package
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\nMaintainer: Jane Smith <jane@example.com>\n\nPackage: test-package\nDescription: Test package\n Test description\n",
        );
        assert_eq!(
            result.description,
            "Remove trailing comma from Maintainer field."
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
