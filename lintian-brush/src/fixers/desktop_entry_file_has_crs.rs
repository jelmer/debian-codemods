use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, LintianIssue};
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let debian_dir = base_path.join("debian");
    if !debian_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&debian_dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut diagnostics = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
            continue;
        }

        let content = std::fs::read(&path)?;
        if !content.contains(&b'\r') {
            continue;
        }

        let filename = match path.file_name().and_then(|s| s.to_str()) {
            Some(f) => f.to_string(),
            None => continue,
        };
        let installed_path = format!("usr/share/applications/{}", filename);
        let rel = PathBuf::from("debian").join(&filename);

        for (line_idx, line) in content.split(|&b| b == b'\n').enumerate() {
            if !line.contains(&b'\r') {
                continue;
            }
            let issue = LintianIssue::source_with_info(
                "desktop-entry-file-has-crs",
                vec![format!("[{}:{}]", installed_path, line_idx + 1)],
            );
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
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

fn describe_aggregate(_fixed: &[Diagnostic], _actions: &[Action]) -> String {
    "Remove CRs from desktop files.".to_string()
}

declare_fixer! {
    name: "desktop-entry-file-has-crs",
    tags: ["desktop-entry-file-has-crs"],
    before: ["file-contains-trailing-whitespace"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    },
    describe: |fixed, actions| {
        describe_aggregate(fixed, actions)
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
