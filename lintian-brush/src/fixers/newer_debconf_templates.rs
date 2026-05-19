use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, RunCommandAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// Look up a lintian-brush-internal override from `preferences.extra_env`.
///
/// These knobs (`DEBCONF_GETTEXTIZE_TIMESTAMP`, `DEBCONF_UPDATEPO`) only
/// exist to make this fixer's behaviour deterministic in tests; they are
/// not standard environment variables, so we deliberately do not fall
/// back to the process environment.
fn override_from(preferences: &FixerPreferences, name: &str) -> Option<String> {
    preferences
        .extra_env
        .as_ref()
        .and_then(|e| e.get(name).cloned())
}

pub fn detect(
    ws: &dyn Workspace,
    preferences: &FixerPreferences,
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
    let issue =
        LintianIssue::source_with_info("newer-debconf-templates", Visibility::Warning, info);

    // The debconf-updatepo program; overridable so tests can point at a
    // path that does not exist and exercise the MissingDependency case.
    let updatepo =
        override_from(preferences, "DEBCONF_UPDATEPO").unwrap_or_else(|| "debconf-updatepo".into());

    // If a deterministic timestamp is requested, run debconf-updatepo and
    // rewrite POT-Creation-Date in any .po file it touched. We delegate
    // both to the shell so the applier sees a single command.
    let argv = if let Some(timestamp_str) =
        override_from(preferences, "DEBCONF_GETTEXTIZE_TIMESTAMP")
    {
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
             {}\n\
             for f in debian/po/*; do\n\
                 [ -f \"$f\" ] || continue\n\
                 if ! cmp -s \"$f\" \"$before/$(basename \"$f\")\" 2>/dev/null; then\n\
                     sed -i \"s|^\\\"POT-Creation-Date: .*|\\\"POT-Creation-Date: {}\\\\\\\\n\\\"|\" \"$f\"\n\
                 fi\n\
             done\n\
             rm -rf \"$before\"\n",
            updatepo, formatted
        );
        vec!["sh".into(), "-c".into(), script]
    } else {
        vec![updatepo]
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Debconf templates are newer than translations.".to_string(),
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
    triggers: [
        debian_workspace::Trigger::Glob("debian/po/*"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply_with(
        base: &Path,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(v.clone()),
        );
        adapter.apply(&ws, preferences)
    }

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        run_apply_with(base, &FixerPreferences::default())
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

        // Point debconf-updatepo at a path that does not exist instead of
        // mutating the process PATH, which would race other tests.
        let missing = tmp.path().join("nonexistent-debconf-updatepo");
        let mut extra_env = std::collections::HashMap::new();
        extra_env.insert(
            "DEBCONF_UPDATEPO".to_string(),
            missing.to_str().unwrap().to_string(),
        );
        let preferences = FixerPreferences {
            extra_env: Some(extra_env),
            ..Default::default()
        };

        match run_apply_with(tmp.path(), &preferences) {
            Err(FixerError::MissingDependency(name)) => {
                assert_eq!(name, missing.to_str().unwrap())
            }
            other => panic!("expected MissingDependency, got {:?}", other),
        }
    }
}
