use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{FixerError, LintianIssue};
use std::fs;
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_path = base_path.join(&control_rel);
    if !control_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&control_path)?;
    if !content.contains("\r\n") {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "carriage-return-line-feed",
        vec!["debian/control".to_string()],
    );

    let converted = content.replace("\r\n", "\n");
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Format control file with unix-style line endings.",
        vec![Action::Filesystem(FilesystemAction::Write {
            file: control_rel,
            content: converted.into_bytes(),
        })],
    )])
}

declare_fixer! {
    name: "control-file-with-CRLF-EOLs",
    tags: ["carriage-return-line-feed"],
    // Must normalize line endings before whitespace cleanup to avoid corrupting content
    before: ["file-contains-trailing-whitespace"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use tempfile::tempdir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_crlf_control_converted() {
        let temp_dir = tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();
        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test-package\r\nSection: misc\r\n").unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Format control file with unix-style line endings."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\nSection: misc\n",
        );
    }

    #[test]
    fn test_no_change_when_already_lf() {
        let temp_dir = tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();
        let control_path = debian_dir.join("control");
        let original = "Source: test-package\nSection: misc\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = tempdir().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_mixed_line_endings() {
        let temp_dir = tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();
        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: a\r\nSection: b\nPriority: c\r\n").unwrap();

        run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: a\nSection: b\nPriority: c\n",
        );
    }
}
