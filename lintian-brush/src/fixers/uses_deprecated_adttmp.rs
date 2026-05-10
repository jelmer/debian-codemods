use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, FilesystemAction};
use debian_workspace::Workspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use regex::bytes::Regex;
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut entries = match ws.list_dir(Path::new("debian/tests"))? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    entries.sort();

    let pattern = Regex::new(r"\bADTTMP\b").unwrap();
    let mut diagnostics = Vec::new();

    for name in entries {
        let rel = PathBuf::from("debian/tests").join(&name);
        let Some(content) = ws.read_file(&rel)? else {
            continue;
        };

        let mut line_numbers = Vec::new();
        for (line_num, line) in content.split(|&b| b == b'\n').enumerate() {
            if pattern.is_match(line) {
                line_numbers.push(line_num + 1);
            }
        }
        if line_numbers.is_empty() {
            continue;
        }

        let rel_str = rel.to_string_lossy().to_string();

        for line_num in line_numbers {
            let issue = LintianIssue::source_with_info(
                "uses-deprecated-adttmp",
                Visibility::Warning,
                vec![format!("[{}:{}]", rel_str, line_num)],
            );
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    "Test script uses deprecated $ADTTMP.".to_string(),
                    "Replace use of deprecated $ADTTMP with $AUTOPKGTEST_TMP.".to_string(),
                    vec![Action::Filesystem(FilesystemAction::Substitute {
                        file: rel.clone(),
                        from: "ADTTMP".into(),
                        to: "AUTOPKGTEST_TMP".into(),
                    })],
                )
                .with_certainty(Certainty::Certain),
            );
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    "Replace use of deprecated $ADTTMP with $AUTOPKGTEST_TMP.".to_string()
}

declare_detector! {
    name: "uses-deprecated-adttmp",
    tags: ["uses-deprecated-adttmp"],
    triggers: [debian_workspace::Trigger::Glob("debian/tests/*")],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_replaces_adttmp() {
        let tmp = TempDir::new().unwrap();
        let tests_dir = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();
        let test_file = tests_dir.join("athing");
        fs::write(&test_file, b"#!/bin/sh\n\ntouch $ADTTMP/blah\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&test_file).unwrap(),
            "#!/bin/sh\n\ntouch $AUTOPKGTEST_TMP/blah\n"
        );
    }

    #[test]
    fn test_no_change_when_no_adttmp() {
        let tmp = TempDir::new().unwrap();
        let tests_dir = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();
        fs::write(
            tests_dir.join("athing"),
            b"#!/bin/sh\n\ntouch $AUTOPKGTEST_TMP/blah\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_tests_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_files() {
        let tmp = TempDir::new().unwrap();
        let tests_dir = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();
        fs::write(tests_dir.join("test1"), b"echo $ADTTMP\n").unwrap();
        fs::write(tests_dir.join("test2"), b"cd $ADTTMP && ls\n").unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(tests_dir.join("test1")).unwrap(),
            "echo $AUTOPKGTEST_TMP\n"
        );
        assert_eq!(
            fs::read_to_string(tests_dir.join("test2")).unwrap(),
            "cd $AUTOPKGTEST_TMP && ls\n"
        );
    }
}
