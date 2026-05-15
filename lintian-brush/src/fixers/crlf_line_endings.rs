use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

const CONTROL_REL: &str = "debian/control";

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let bytes = match ws.read_file(Path::new(CONTROL_REL))? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    // Operate on bytes rather than UTF-8 strings so an embedded non-UTF-8
    // byte (rare but possible in Debian control files in the wild) doesn't
    // mask a CRLF.
    if !bytes.windows(2).any(|w| w == b"\r\n") {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "carriage-return-line-feed",
        Visibility::Error,
        vec![CONTROL_REL.to_string()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "debian/control uses CRLF line endings.",
        "Format control file with unix-style line endings.",
        vec![Action::Filesystem(FilesystemAction::NormalizeLineEndings {
            file: PathBuf::from(CONTROL_REL),
        })],
    )])
}

declare_detector! {
    name: "control-file-with-CRLF-EOLs",
    tags: ["carriage-return-line-feed"],
    // Must normalize line endings before whitespace cleanup to avoid corrupting content.
    before: ["file-contains-trailing-whitespace"],
    triggers: [debian_workspace::Trigger::File("debian/control")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use tempfile::tempdir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, Some("test".into()), Some("1.0".parse().unwrap()));
        detect(&ws, &FixerPreferences::default())
    }

    #[test]
    fn detect_emits_normalize_action_when_crlf_present() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\r\nSection: misc\r\n",
        )
        .unwrap();

        let diags = detect_in(tmp.path()).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].plans[0].actions[0],
            Action::Filesystem(FilesystemAction::NormalizeLineEndings {
                file: PathBuf::from("debian/control"),
            })
        );
    }

    #[test]
    fn apply_rewrites_control_with_crlf() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\r\nSection: misc\r\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Format control file with unix-style line endings."
        );
        let content = fs::read_to_string(debian.join("control")).unwrap();
        assert_eq!(content, "Source: test-package\nSection: misc\n");
    }

    #[test]
    fn no_changes_when_lf_only() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nSection: misc\n",
        )
        .unwrap();

        assert!(detect_in(tmp.path()).unwrap().is_empty());
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn no_changes_when_no_control_file() {
        let tmp = tempdir().unwrap();
        // No debian/ at all.
        assert!(detect_in(tmp.path()).unwrap().is_empty());
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
