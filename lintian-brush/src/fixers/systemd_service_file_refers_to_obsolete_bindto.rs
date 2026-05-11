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
        let Some(unit_section) = unit.get_section("Unit") else {
            continue;
        };
        if unit_section.get_all("BindTo").is_empty() {
            continue;
        }

        let rel_str = rel.to_string_lossy().to_string();

        let issue = LintianIssue::source_with_info(
            "systemd-service-file-refers-to-obsolete-bindto",
            Visibility::Warning,
            vec![rel_str],
        );

        diagnostics.push(Diagnostic::with_actions(
            issue,
            "systemd service file refers to obsolete BindTo key.",
            "Rename BindTo key to BindsTo in systemd files.",
            vec![Action::Systemd(SystemdAction::RenameField {
                file: rel.clone(),
                section: "Unit".into(),
                from: "BindTo".into(),
                to: "BindsTo".into(),
            })],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "systemd-service-file-refers-to-obsolete-bindto",
    tags: ["systemd-service-file-refers-to-obsolete-bindto"],
    triggers: [debian_workspace::Trigger::Glob("debian/*.service")],
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
    fn test_rename_bindto_to_bindsto() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\nBindTo=foo.service\n\n[Service]\nType=oneshot\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test Service\nBindsTo=foo.service\n\n[Service]\nType=oneshot\n",
        );
    }

    #[test]
    fn test_rename_multiple_bindto_entries() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\nBindTo=foo.service\nBindTo=bar.service\n\n[Service]\nType=oneshot\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test Service\nBindsTo=foo.service\nBindsTo=bar.service\n\n[Service]\nType=oneshot\n",
        );
    }

    #[test]
    fn test_no_bindto_entries() {
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
    fn test_existing_bindsto_not_affected() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\nBindsTo=existing.service\nBindTo=new.service\n\n[Service]\nType=oneshot\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test Service\nBindsTo=existing.service\nBindsTo=new.service\n\n[Service]\nType=oneshot\n",
        );
    }
}
