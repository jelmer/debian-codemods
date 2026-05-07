use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let content = match ws.read_file(Path::new("debian/rules"))? {
        Some(c) => c,
        None => return Ok(Vec::new()),
    };
    if !content
        .windows(b"$(dir $(_))".len())
        .any(|w| w == b"$(dir $(_))")
    {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "debian-rules-uses-special-shell-variable",
        vec!["[debian/rules]".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Avoid using $(_) to discover source package directory.".to_string(),
        "Avoid using $(_) to discover source package directory.".to_string(),
        vec![Action::Filesystem(FilesystemAction::Substitute {
            file: rules_rel,
            from: "$(dir $(_))".into(),
            to: "$(dir $(firstword $(MAKEFILE_LIST)))".into(),
        })],
    )])
}

declare_detector! {
    name: "debian-rules-uses-special-shell-variable",
    tags: ["debian-rules-uses-special-shell-variable"],
    triggers: [crate::workspace::Trigger::File("debian/rules")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_replace_special_shell_variable() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("rules");
        fs::write(
            &path,
            "#!/usr/bin/make -f\n\n%:\n\tdh $*\n\nget-orig-source:\n\tuscan --watchfile=$(dir $(_))/watch\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Avoid using $(_) to discover source package directory."
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $*\n\nget-orig-source:\n\tuscan --watchfile=$(dir $(firstword $(MAKEFILE_LIST)))/watch\n",
        );
    }

    #[test]
    fn test_no_change_when_not_present() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("rules"), "#!/usr/bin/make -f\n\n%:\n\tdh $*\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
