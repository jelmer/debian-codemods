use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let content = match ws.read_file(&copyright_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    if !content.contains(&b'\r') {
        return Ok(Vec::new());
    }

    let new_content: Vec<u8> = content.iter().copied().filter(|&b| b != b'\r').collect();

    let issue = LintianIssue::source("copyright-has-crs", Visibility::Pedantic);
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Remove CRs from copyright file.".to_string(),
        "Remove CRs from copyright file.".to_string(),
        vec![Action::Filesystem(FilesystemAction::Write {
            file: copyright_rel,
            content: new_content,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "copyright-has-crs",
    tags: ["copyright-has-crs"],
    // Must normalize line endings before whitespace cleanup to avoid
    // corrupting content.
    before: ["file-contains-trailing-whitespace"],
    triggers: [crate::workspace::Trigger::File("debian/copyright")],
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
