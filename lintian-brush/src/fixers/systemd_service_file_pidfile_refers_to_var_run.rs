use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, SystemdAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};
use std::str::FromStr;

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
        if !filename.ends_with(".service") {
            continue;
        }
        let rel = PathBuf::from("debian").join(&filename);
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let unit = systemd_unit_edit::SystemdUnit::from_str(&content).map_err(|e| {
            FixerError::Other(format!("Failed to parse {}: {:?}", rel.display(), e))
        })?;
        let Some(service_section) = unit.get_section("Service") else {
            continue;
        };
        let Some(old_pidfile) = service_section.get("PIDFile") else {
            continue;
        };
        if !old_pidfile.contains("/var/run/") {
            continue;
        }
        let new_pidfile = old_pidfile.replace("/var/run/", "/run/");

        let rel_str = rel.to_string_lossy().to_string();

        let issue = LintianIssue::source_with_info(
            "systemd-service-file-refers-to-var-run",
            Visibility::Info,
            vec![rel_str.clone(), "PIDFile".to_string(), old_pidfile.clone()],
        );

        // Set PIDFile, then sweep every other field in [Service] that
        // embedded the old pidfile path (e.g. ExecStart=...
        // --pidfile=/var/run/foo.pid).
        let mut actions = vec![Action::Systemd(SystemdAction::SetField {
            file: rel.clone(),
            section: "Service".into(),
            field: "PIDFile".into(),
            value: new_pidfile.clone(),
        })];
        for e in service_section.entries() {
            let (Some(key), Some(value)) = (e.key(), e.value()) else {
                continue;
            };
            if key == "PIDFile" {
                continue;
            }
            if !value.contains(&old_pidfile) {
                continue;
            }
            actions.push(Action::Systemd(SystemdAction::SetField {
                file: rel.clone(),
                section: "Service".into(),
                field: key,
                value: value.replace(&old_pidfile, &new_pidfile),
            }));
        }

        diagnostics.push(Diagnostic::with_actions(
            issue,
            "systemd service PIDFile refers to obsolete /var/run.",
            "Replace /var/run with /run for the Service PIDFile.",
            actions,
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "systemd-service-file-pidfile-refers-to-var-run",
    tags: ["systemd-service-file-refers-to-var-run"],
    triggers: [debian_workspace::Trigger::Glob("debian/*.service")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
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
    fn test_replace_var_run_in_pidfile() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\n\n[Service]\nType=forking\nPIDFile=/var/run/test.pid\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test Service\n\n[Service]\nType=forking\nPIDFile=/run/test.pid\n",
        );
    }

    #[test]
    fn test_replace_var_run_in_execstart_too() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\n\n[Service]\nExecStart=/sbin/daemon --pidfile=/var/run/test.pid\nType=forking\nPIDFile=/var/run/test.pid\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test Service\n\n[Service]\nExecStart=/sbin/daemon --pidfile=/run/test.pid\nType=forking\nPIDFile=/run/test.pid\n",
        );
    }

    #[test]
    fn test_no_var_run_unchanged() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        let original =
            "[Unit]\nDescription=Test Service\n\n[Service]\nType=forking\nPIDFile=/run/test.pid\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_pidfile() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\n\n[Service]\nType=simple\nExecStart=/sbin/daemon\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
