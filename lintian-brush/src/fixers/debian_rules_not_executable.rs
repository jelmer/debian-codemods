use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let mode = match ws.file_mode(Path::new("debian/rules"))? {
        Some(m) => m,
        None => return Ok(Vec::new()),
    };
    if (mode & 0o111) != 0 {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "debian-rules-not-executable",
        Visibility::Pedantic,
        vec!["debian/rules".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Make debian/rules executable.".to_string(),
        "Make debian/rules executable.".to_string(),
        vec![Action::Filesystem(FilesystemAction::SetMode {
            file: rules_rel,
            mode: 0o755,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "debian-rules-not-executable",
    tags: ["debian-rules-not-executable"],
    triggers: [debian_workspace::Trigger::File("debian/rules")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
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
    fn test_make_rules_executable() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(&rules, "#!/usr/bin/make -f\n%:\n\tdh $@\n").unwrap();
        let mut perms = fs::metadata(&rules).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&rules, perms).unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, "Make debian/rules executable.");
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(
            fs::metadata(&rules).unwrap().permissions().mode() & 0o777,
            0o755,
        );
    }

    #[test]
    fn test_rules_already_executable() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(&rules, "#!/usr/bin/make -f\n%:\n\tdh $@\n").unwrap();
        let mut perms = fs::metadata(&rules).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&rules, perms).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
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
