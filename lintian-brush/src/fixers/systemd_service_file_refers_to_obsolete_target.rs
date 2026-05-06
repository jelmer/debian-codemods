use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, SystemdAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::{Path, PathBuf};
use std::str::FromStr;

const DEPRECATED_TARGETS: &[&str] = &["syslog.target"];

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
        let after_values = unit_section.get_all("After");

        let rel_str = rel.to_string_lossy().to_string();

        for &target in DEPRECATED_TARGETS {
            // After= values can be space-separated; we only fire if at
            // least one entry mentions the target.
            let mentions = after_values
                .iter()
                .any(|v| v.split_whitespace().any(|t| t == target));
            if !mentions {
                continue;
            }

            let issue = LintianIssue::source_with_info(
                "systemd-service-file-refers-to-obsolete-target",
                vec![format!("{} {}", rel_str, target)],
            );

            diagnostics.push(Diagnostic::with_actions(
                issue,
                "Remove references to obsolete targets in systemd unit files.",
                vec![Action::Systemd(SystemdAction::RemoveValue {
                    file: rel.clone(),
                    section: "Unit".into(),
                    field: "After".into(),
                    value: target.to_string(),
                })],
            ));
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "systemd-service-file-refers-to-obsolete-target",
    tags: ["systemd-service-file-refers-to-obsolete-target"],
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
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_syslog_target_from_after() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\nAfter=syslog.target\n\n[Service]\nType=oneshot\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test Service\n\n[Service]\nType=oneshot\n",
        );
    }

    #[test]
    fn test_remove_syslog_target_from_multi_value() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\nAfter=network.target syslog.target\n\n[Service]\nType=oneshot\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test Service\nAfter=network.target\n\n[Service]\nType=oneshot\n",
        );
    }

    #[test]
    fn test_no_syslog_target_unchanged() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        let original =
            "[Unit]\nDescription=Test Service\nAfter=network.target\n\n[Service]\nType=oneshot\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_multiple_after_entries_with_syslog() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\nAfter=network.target\nAfter=syslog.target\n\n[Service]\nType=oneshot\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test Service\nAfter=network.target\n\n[Service]\nType=oneshot\n",
        );
    }
}
