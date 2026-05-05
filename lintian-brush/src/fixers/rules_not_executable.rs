use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let abs = base_path.join(&rules_rel);
    let metadata = match std::fs::metadata(&abs) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mode = metadata.permissions().mode();
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

declare_fixer! {
    name: "rules-not-executable",
    tags: [],
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
