use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, FilesystemAction, TextRange};
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const OLD_REL: &str = "debian/tests/control.autodep8";
const NEW_REL: &str = "debian/tests/control";

const RENAME_TAG: char = 'R';
const MERGE_TAG: char = 'M';

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let old_rel = PathBuf::from(OLD_REL);
    let new_rel = PathBuf::from(NEW_REL);
    let old_bytes = match ws.read_file(&old_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };

    let issue_obsolete = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        visibility: Some(Visibility::Warning),
        tag: Some("debian-tests-control-autodep8-is-obsolete".to_string()),
        info: Some(OLD_REL.to_string()),
    };

    let new_bytes = match ws.read_file(&new_rel)? {
        Some(b) => b,
        None => {
            // Simple rename.
            return Ok(vec![Diagnostic::with_actions(
                issue_obsolete,
                format!("{}", RENAME_TAG),
                format!("Rename {} to {}.", OLD_REL, NEW_REL),
                vec![Action::Filesystem(FilesystemAction::Rename {
                    file: old_rel,
                    to: new_rel,
                })],
            )]);
        }
    };

    // Both files exist: append the autodep8 contents to the existing
    // control file (with a separating newline) and delete the autodep8
    // file. The detector reads the source bytes; the actions express the
    // resulting edits without re-reading.

    let mut suffix = Vec::with_capacity(1 + old_bytes.len());
    suffix.push(b'\n');
    suffix.extend_from_slice(&old_bytes);
    let suffix_str = String::from_utf8(suffix)
        .map_err(|e| FixerError::Other(format!("autodep8 contents are not valid UTF-8: {}", e)))?;

    let issue_merge = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        visibility: Some(Visibility::Warning),
        tag: Some("debian-tests-control-and-control-autodep8".to_string()),
        info: Some(format!("{} {}", OLD_REL, NEW_REL)),
    };

    let actions = vec![
        Action::Filesystem(FilesystemAction::ReplaceText {
            file: new_rel,
            range: TextRange {
                start: new_bytes.len(),
                end: new_bytes.len(),
            },
            replacement: suffix_str,
        }),
        Action::Filesystem(FilesystemAction::Delete { file: old_rel }),
    ];

    // The issue order in the message file is merge first, then obsolete.
    Ok(vec![
        Diagnostic::with_actions(
            issue_merge,
            format!("{}", MERGE_TAG),
            format!("Merge {} into {}.", OLD_REL, NEW_REL),
            actions,
        ),
        // The obsolete diagnostic carries no actions of its own — the
        // merge above already removed the file. Emitting it as a separate
        // diagnostic ensures the lintian issue is still reported.
        Diagnostic::with_actions(
            issue_obsolete,
            format!("{}", MERGE_TAG),
            format!("Merge {} into {}.", OLD_REL, NEW_REL),
            vec![Action::Filesystem(FilesystemAction::Delete {
                file: PathBuf::from(OLD_REL),
            })],
        ),
    ])
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let is_merge = fixed.iter().any(|(d, _)| d.message.starts_with(MERGE_TAG));
    if is_merge {
        format!("Merge {} into {}.", OLD_REL, NEW_REL)
    } else {
        format!("Rename obsolete path {} to {}.", OLD_REL, NEW_REL)
    }
}

declare_detector! {
    name: "debian-tests-control-autodep8-is-obsolete",
    tags: ["debian-tests-control-autodep8-is-obsolete", "debian-tests-control-and-control-autodep8"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/tests/control.autodep8",
            paragraph_key: "Tests",
            field: "*",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/tests/control.autodep8",
            paragraph_key: "Test-Command",
            field: "*",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/tests/control",
            paragraph_key: "Tests",
            field: "*",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/tests/control",
            paragraph_key: "Test-Command",
            field: "*",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
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
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_renames_autodep8_file() {
        let tmp = TempDir::new().unwrap();
        let tests_dir = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();
        let old_path = tests_dir.join("control.autodep8");
        fs::write(&old_path, "Test-Command: echo test\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(!old_path.exists());
        assert_eq!(
            fs::read_to_string(tests_dir.join("control")).unwrap(),
            "Test-Command: echo test\n",
        );
    }

    #[test]
    fn test_merges_when_both_exist() {
        let tmp = TempDir::new().unwrap();
        let tests_dir = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();
        let old_path = tests_dir.join("control.autodep8");
        let new_path = tests_dir.join("control");
        fs::write(&old_path, "Test-Command: echo old\n").unwrap();
        fs::write(&new_path, "Test-Command: echo new\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(!old_path.exists());
        assert_eq!(
            fs::read_to_string(&new_path).unwrap(),
            "Test-Command: echo new\n\nTest-Command: echo old\n",
        );
    }

    #[test]
    fn test_no_change_when_no_autodep8() {
        let tmp = TempDir::new().unwrap();
        let tests_dir = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();
        fs::write(tests_dir.join("control"), "Test-Command: echo test\n").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_tests_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
