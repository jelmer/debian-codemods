use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use debian_workspace::Workspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use makefile_lossless::Makefile;
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let bytes = match ws.read_file(Path::new("debian/rules"))? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };
    // Parse as a sanity check; if the file isn't a parseable makefile
    // there's nothing safe to substitute in.
    if Makefile::read_relaxed(content.as_bytes()).is_err() {
        return Ok(Vec::new());
    }
    if !content.contains("$(PWD)") {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "debian-rules-calls-pwd",
        Visibility::Warning,
        vec!["[debian/rules]".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "debian/rules: Avoid using $(PWD) variable.".to_string(),
        "debian/rules: Avoid using $(PWD) variable.".to_string(),
        vec![Action::Filesystem(FilesystemAction::Substitute {
            file: rules_rel,
            from: "$(PWD)".into(),
            to: "$(CURDIR)".into(),
        })],
    )])
}

declare_detector! {
    name: "debian-rules-should-not-use-pwd",
    tags: ["debian-rules-calls-pwd"],
    triggers: [debian_workspace::Trigger::File("debian/rules")],
    detect: |ws, prefs| detect(ws, prefs),
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
    fn test_replace_pwd_with_curdir() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("rules");
        fs::write(
            &path,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_auto_install:\n\tdh_auto_install --destdir=$(PWD)/debian/tmp\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "debian/rules: Avoid using $(PWD) variable."
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_auto_install:\n\tdh_auto_install --destdir=$(CURDIR)/debian/tmp\n",
        );
    }

    #[test]
    fn test_multiple_pwd_occurrences() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("rules");
        fs::write(
            &path,
            "#!/usr/bin/make -f\n\nFOO=$(PWD)/foo\nBAR=$(PWD)/bar\n\n%:\n\tdh $@\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\nFOO=$(CURDIR)/foo\nBAR=$(CURDIR)/bar\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_no_pwd_in_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_auto_install:\n\tdh_auto_install --destdir=$(CURDIR)/debian/tmp\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_pwd_in_command() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("rules");
        fs::write(
            &path,
            "#!/usr/bin/make -f\n\ntest:\n\techo $(PWD)\n\tcp $(PWD)/file dest/\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\ntest:\n\techo $(CURDIR)\n\tcp $(CURDIR)/file dest/\n",
        );
    }

    #[test]
    fn test_pwd_in_variable_and_command() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("rules");
        fs::write(
            &path,
            "#!/usr/bin/make -f\n\nBUILDDIR=$(PWD)/build\n\noverride_dh_auto_configure:\n\tdh_auto_configure -- --prefix=$(PWD)/debian/tmp\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\nBUILDDIR=$(CURDIR)/build\n\noverride_dh_auto_configure:\n\tdh_auto_configure -- --prefix=$(CURDIR)/debian/tmp\n",
        );
    }
}
