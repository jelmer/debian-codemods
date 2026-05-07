use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, RunCommandAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let po_dir_rel = PathBuf::from("debian/po");
    let entries = match ws.list_dir(&po_dir_rel)? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };

    let info = if entries.iter().any(|n| n == "templates.pot") {
        vec!["[debian/po/templates.pot]".to_string()]
    } else {
        vec![]
    };
    let issue = LintianIssue::source_with_info("newer-debconf-templates", info);

    // If a deterministic timestamp is requested, run debconf-updatepo and
    // rewrite POT-Creation-Date in any .po file it touched. We delegate
    // both to the shell so the applier sees a single command.
    let argv = if let Ok(timestamp_str) = std::env::var("DEBCONF_GETTEXTIZE_TIMESTAMP") {
        let timestamp: i64 = timestamp_str
            .parse()
            .map_err(|_| FixerError::Other("Invalid DEBCONF_GETTEXTIZE_TIMESTAMP".to_string()))?;
        let dt = chrono::DateTime::from_timestamp(timestamp, 0)
            .ok_or_else(|| FixerError::Other("Invalid timestamp".to_string()))?;
        let formatted = dt.format("%Y-%m-%d %H:%M+0000").to_string();
        let script = format!(
            "set -e\n\
             before=$(mktemp -d)\n\
             cp -a debian/po/. \"$before\"/\n\
             debconf-updatepo\n\
             for f in debian/po/*; do\n\
                 [ -f \"$f\" ] || continue\n\
                 if ! cmp -s \"$f\" \"$before/$(basename \"$f\")\" 2>/dev/null; then\n\
                     sed -i \"s|^\\\"POT-Creation-Date: .*|\\\"POT-Creation-Date: {}\\\\\\\\n\\\"|\" \"$f\"\n\
                 fi\n\
             done\n\
             rm -rf \"$before\"\n",
            formatted
        );
        vec!["sh".into(), "-c".into(), script]
    } else {
        vec!["debconf-updatepo".into()]
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Run debconf-updatepo after template changes.".to_string(),
        vec![Action::RunCommand(RunCommandAction::Run {
            argv,
            scope: po_dir_rel,
            env: Vec::new(),
        })],
    )])
}

declare_detector! {
    name: "newer-debconf-templates",
    tags: ["newer-debconf-templates"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_no_po_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_missing_debconf_updatepo() {
        // When the binary isn't installed, we expect MissingDependency.
        let tmp = TempDir::new().unwrap();
        let po_dir = tmp.path().join("debian/po");
        fs::create_dir_all(&po_dir).unwrap();
        fs::write(po_dir.join("POTFILES.in"), "").unwrap();

        // Set PATH to an empty directory so debconf-updatepo isn't found.
        let empty_bin = tmp.path().join("empty-bin");
        fs::create_dir(&empty_bin).unwrap();
        let old_path = std::env::var("PATH").unwrap_or_default();
        // SAFETY: the test process is single-threaded for this assertion.
        unsafe {
            std::env::set_var("PATH", empty_bin.to_str().unwrap());
        }

        let result = run_apply(tmp.path());

        unsafe {
            std::env::set_var("PATH", old_path);
        }

        match result {
            Err(FixerError::MissingDependency(name)) => assert_eq!(name, "debconf-updatepo"),
            other => panic!("expected MissingDependency, got {:?}", other),
        }
    }
}
