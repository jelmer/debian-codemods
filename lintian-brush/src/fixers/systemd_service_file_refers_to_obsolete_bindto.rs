use crate::diagnostic::{Action, Diagnostic, SystemdAction};
use crate::{FixerError, LintianIssue};
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let debian_path = base_path.join("debian");
    if !debian_path.exists() {
        return Ok(Vec::new());
    }

    let mut diagnostics = Vec::new();
    for entry in std::fs::read_dir(&debian_path)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "service") {
            continue;
        }
        if path.is_symlink() {
            continue;
        }

        let content = std::fs::read_to_string(&path)?;
        let unit = systemd_unit_edit::SystemdUnit::from_str(&content).map_err(|e| {
            FixerError::Other(format!("Failed to parse {}: {:?}", path.display(), e))
        })?;
        let Some(unit_section) = unit.get_section("Unit") else {
            continue;
        };
        if unit_section.get_all("BindTo").is_empty() {
            continue;
        }

        let rel: PathBuf = path.strip_prefix(base_path).unwrap_or(&path).to_path_buf();
        let rel_str = rel.to_string_lossy().to_string();

        let issue = LintianIssue::source_with_info(
            "systemd-service-file-refers-to-obsolete-bindto",
            vec![rel_str],
        );

        diagnostics.push(Diagnostic::with_actions(
            issue,
            "Rename BindTo key to BindsTo in systemd files.",
            vec![Action::Systemd(SystemdAction::RenameField {
                file: rel,
                section: "Unit".into(),
                from: "BindTo".into(),
                to: "BindsTo".into(),
            })],
        ));
    }

    Ok(diagnostics)
}

declare_fixer! {
    name: "systemd-service-file-refers-to-obsolete-bindto",
    tags: ["systemd-service-file-refers-to-obsolete-bindto"],
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
