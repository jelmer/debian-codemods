use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, LintianIssue};
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let abs = base_path.join(&copyright_rel);
    if !abs.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read(&abs)?;
    if !content.contains(&b'\r') {
        return Ok(Vec::new());
    }

    let new_content: Vec<u8> = content.into_iter().filter(|&b| b != b'\r').collect();

    let issue = LintianIssue::source("copyright-has-crs");
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Remove CRs from copyright file.".to_string(),
        vec![Action::Filesystem(FilesystemAction::Write {
            file: copyright_rel,
            content: new_content,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_fixer! {
    name: "copyright-has-crs",
    tags: ["copyright-has-crs"],
    // Must normalize line endings before whitespace cleanup to avoid
    // corrupting content.
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
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_crs() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            b"Format: example\r\nUpstream-Name: test\r\n\r\nFiles: *\r\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, "Remove CRs from copyright file.");
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag,
            Some("copyright-has-crs".to_string())
        );
        assert_eq!(result.fixed_lintian_issues[0].info, None);

        assert_eq!(
            fs::read(&path).unwrap(),
            b"Format: example\nUpstream-Name: test\n\nFiles: *\n",
        );
    }

    #[test]
    fn test_no_crs() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            b"Format: example\nUpstream-Name: test\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
