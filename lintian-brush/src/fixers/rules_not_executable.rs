use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences};
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let mode = match ws.file_mode(Path::new("debian/rules"))? {
        Some(m) => m,
        None => return Ok(Vec::new()),
    };
    if mode & 0o111 != 0 {
        return Ok(Vec::new());
    }

    Ok(vec![Diagnostic::untagged(
        "Mark debian/rules as executable.",
        vec![Action::Filesystem(FilesystemAction::SetMode {
            file: rules_rel,
            mode: 0o755,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "rules-not-executable",
    tags: [],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
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
        assert_eq!(result.description, "Mark debian/rules as executable.");
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(
            fs::metadata(&rules).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }

    #[test]
    fn test_already_executable() {
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
