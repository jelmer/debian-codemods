use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, FilesystemAction};
use debian_workspace::Workspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut entries = match ws.list_dir(Path::new("debian"))? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    entries.sort();

    let mut diagnostics = Vec::new();
    for filename in entries {
        if !filename.ends_with(".desktop") {
            continue;
        }
        let rel = PathBuf::from("debian").join(&filename);
        let Some(content) = ws.read_file(&rel)? else {
            continue;
        };
        if !content.contains(&b'\r') {
            continue;
        }

        let installed_path = format!("usr/share/applications/{}", filename);

        for (line_idx, line) in content.split(|&b| b == b'\n').enumerate() {
            if !line.contains(&b'\r') {
                continue;
            }
            let issue = LintianIssue::source_with_info(
                "desktop-entry-file-has-crs",
                Visibility::Warning,
                vec![format!("[{}:{}]", installed_path, line_idx + 1)],
            );
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    "Desktop entry file contains carriage returns.",
                    "Remove CRs from desktop files.",
                    vec![Action::Filesystem(FilesystemAction::Substitute {
                        file: rel.clone(),
                        from: "\r".into(),
                        to: "".into(),
                    })],
                )
                .with_certainty(Certainty::Certain),
            );
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    "Remove CRs from desktop files.".to_string()
}

declare_detector! {
    name: "desktop-entry-file-has-crs",
    tags: ["desktop-entry-file-has-crs"],
    before: ["file-contains-trailing-whitespace"],
    triggers: [debian_workspace::Trigger::Glob("debian/*.desktop")],
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
    fn test_remove_crs_from_desktop_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let p = debian.join("test.desktop");
        fs::write(&p, b"[Desktop Entry]\r\nType=Application\r\nName=Test\r\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, "Remove CRs from desktop files.");
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(
            fs::read(&p).unwrap(),
            b"[Desktop Entry]\nType=Application\nName=Test\n"
        );
    }

    #[test]
    fn test_multiple_desktop_files() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();

        let d1 = debian.join("app1.desktop");
        let d2 = debian.join("app2.desktop");
        let other = debian.join("control");
        fs::write(&d1, b"[Desktop Entry]\r\nType=Application\r\n").unwrap();
        fs::write(&d2, b"[Desktop Entry]\nType=Service\n").unwrap();
        fs::write(&other, b"Source: test\r\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(!fs::read(&d1).unwrap().contains(&b'\r'));
        assert!(!fs::read(&d2).unwrap().contains(&b'\r'));
        assert!(fs::read(&other).unwrap().contains(&b'\r'));
    }

    #[test]
    fn test_no_desktop_files_with_crs() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("test.desktop"),
            b"[Desktop Entry]\nType=Application\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_desktop_files() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("control"), b"Source: test\r\n").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
