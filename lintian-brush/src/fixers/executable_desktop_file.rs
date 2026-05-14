use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
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
        let Some(current_mode) = ws.file_mode(&rel)? else {
            continue;
        };
        if (current_mode & 0o111) == 0 {
            continue;
        }

        let installed_path = format!("usr/share/applications/{}", filename);
        let perms_octal = format!("{:04o}", current_mode & 0o777);

        let issue = LintianIssue::source_with_info(
            "executable-desktop-file",
            Visibility::Error,
            vec![format!("{} [{}]", perms_octal, installed_path)],
        );

        diagnostics.push(Diagnostic::with_actions(
            issue,
            "Desktop file is executable.",
            "Remove executable bit from desktop files.",
            vec![Action::Filesystem(FilesystemAction::SetMode {
                file: rel,
                mode: current_mode & !0o111,
            })],
        ));
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "executable-desktop-file",
    tags: ["executable-desktop-file"],
    triggers: [debian_workspace::Trigger::Glob("debian/*.desktop")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn test_remove_executable_bit_via_apply() {
        let temp_dir = tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();
        let path = debian_dir.join("test.desktop");
        fs::write(&path, "[Desktop Entry]\nName=App").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove executable bit from desktop files."
        );
        assert_eq!(mode_of(&path), 0o644);
    }

    #[test]
    fn test_no_change_when_already_non_executable() {
        let temp_dir = tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();
        let path = debian_dir.join("test.desktop");
        fs::write(&path, "[Desktop Entry]\nName=App").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(mode_of(&path), 0o644);
    }

    #[test]
    fn test_multiple_desktop_files() {
        let temp_dir = tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();
        let p1 = debian_dir.join("a.desktop");
        let p2 = debian_dir.join("b.desktop");
        fs::write(&p1, "[Desktop Entry]\nName=A").unwrap();
        fs::write(&p2, "[Desktop Entry]\nName=B").unwrap();
        fs::set_permissions(&p1, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&p2, fs::Permissions::from_mode(0o711)).unwrap();

        run_apply(temp_dir.path()).unwrap();
        assert_eq!(mode_of(&p1), 0o644);
        assert_eq!(mode_of(&p2), 0o600);
    }

    #[test]
    fn test_no_debian_dir() {
        let temp_dir = tempdir().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
