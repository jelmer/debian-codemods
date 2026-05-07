use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, SystemdAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Check if a space-separated list contains a specific item.
fn list_contains(value: &str, item: &str) -> bool {
    value.split_whitespace().any(|v| v == item)
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut entries = match ws.list_dir(Path::new("debian"))? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    entries.sort();

    let mut diagnostics = Vec::new();
    for filename in entries {
        if !filename.ends_with(".service") {
            continue;
        }
        let rel = PathBuf::from("debian").join(&filename);
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        let unit = systemd_unit_edit::SystemdUnit::from_str(&content).map_err(|e| {
            FixerError::Other(format!("Failed to parse {}: {:?}", rel.display(), e))
        })?;
        let Some(unit_section) = unit.get_section("Unit") else {
            continue;
        };

        // Trigger only when the unit shuts down on its own (DefaultDependencies=no)
        // and conflicts with shutdown.target but doesn't already declare
        // Before=shutdown.target — see systemd.unit(5) for the rationale.
        let default_deps_no = unit_section.get("DefaultDependencies").as_deref() == Some("no");
        let conflicts_shutdown = unit_section
            .get("Conflicts")
            .as_ref()
            .is_some_and(|c| list_contains(c, "shutdown.target"));
        let already_before = unit_section
            .get_all("Before")
            .iter()
            .any(|b| list_contains(b, "shutdown.target"));
        if !(default_deps_no && conflicts_shutdown && !already_before) {
            continue;
        }

        let rel_str = rel.to_string_lossy().to_string();
        let issue =
            LintianIssue::source_with_info("systemd-service-file-shutdown-problems", vec![rel_str]);

        diagnostics.push(Diagnostic::with_actions(
            issue,
            "Add Before=shutdown.target to Unit section.",
            vec![Action::Systemd(SystemdAction::Add {
                file: rel.clone(),
                section: "Unit".into(),
                field: "Before".into(),
                value: "shutdown.target".into(),
            })],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "systemd-service-file-shutdown-problems",
    tags: ["systemd-service-file-shutdown-problems"],
    triggers: [crate::workspace::Trigger::Glob("debian/*.service")],
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
    fn test_list_contains() {
        assert!(list_contains(
            "ssh.service shutdown.target",
            "shutdown.target"
        ));
        assert!(list_contains("shutdown.target", "shutdown.target"));
        assert!(!list_contains("ssh.service", "shutdown.target"));
        assert!(!list_contains("", "shutdown.target"));
    }

    #[test]
    fn test_adds_before_shutdown() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test\nDefaultDependencies=no\nConflicts=shutdown.target\n\n[Service]\nType=oneshot\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test\nDefaultDependencies=no\nConflicts=shutdown.target\nBefore=shutdown.target\n\n[Service]\nType=oneshot\n",
        );
    }

    #[test]
    fn test_no_change_without_default_deps_no() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        let original =
            "[Unit]\nDescription=Test\nConflicts=shutdown.target\n\n[Service]\nType=oneshot\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_change_when_already_before() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        let original = "[Unit]\nDescription=Test\nDefaultDependencies=no\nConflicts=shutdown.target\nBefore=shutdown.target\n\n[Service]\nType=oneshot\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
