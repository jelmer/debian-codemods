use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::workspace::{compat_level, FixerWorkspace};
use crate::{FixerError, FixerPreferences, LintianIssue};
use makefile_lossless::Makefile;
use std::path::{Path, PathBuf};

const DEPRECATED_OVERRIDES: &[&str] = &["override_dh_systemd_enable", "override_dh_systemd_start"];
const NEW_TARGET: &str = "override_dh_installsystemd";

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let rules_bytes = match ws.read_file(Path::new("debian/rules"))? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };

    // Issue only applies to compat level >= 11.
    match compat_level(ws)? {
        Some(level) if level >= 11 => {}
        _ => return Ok(Vec::new()),
    }

    let makefile = Makefile::read_relaxed(rules_bytes.as_slice())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    let new_target_exists = makefile
        .rules()
        .any(|r| r.targets().any(|t| t.trim() == NEW_TARGET));
    if new_target_exists {
        return Ok(Vec::new());
    }

    let mut diagnostics = Vec::new();
    let mut renamed: Vec<String> = Vec::new();
    for rule in makefile.rules() {
        for t in rule.targets() {
            let trimmed = t.trim().to_string();
            if !DEPRECATED_OVERRIDES.contains(&trimmed.as_str()) {
                continue;
            }
            let issue = LintianIssue::source_with_info(
                "debian-rules-uses-deprecated-systemd-override",
                vec![trimmed.clone()],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                String::new(),
                vec![Action::Makefile(MakefileAction::RenameRuleTarget {
                    file: rules_rel.clone(),
                    from_target: trimmed.clone(),
                    to_target: NEW_TARGET.into(),
                })],
            ));
            renamed.push(trimmed);
        }
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    let summary = if renamed.len() == 1 {
        format!("Replace deprecated {} with {}", renamed[0], NEW_TARGET)
    } else {
        format!(
            "Replace deprecated systemd overrides ({}) with {}",
            renamed.join(", "),
            NEW_TARGET
        )
    };
    for d in &mut diagnostics {
        d.message = summary.clone();
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "debian-rules-uses-deprecated-systemd-override",
    tags: ["debian-rules-uses-deprecated-systemd-override"],
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

    fn write_compat(base: &Path, level: u32) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("compat"), format!("{}\n", level)).unwrap();
    }

    #[test]
    fn test_fix_override_dh_systemd_enable() {
        let tmp = TempDir::new().unwrap();
        write_compat(tmp.path(), 11);
        let rules = tmp.path().join("debian/rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_systemd_enable:\n\tdh_systemd_enable --name=myservice\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Replace deprecated override_dh_systemd_enable with override_dh_installsystemd"
        );
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_installsystemd:\n\tdh_systemd_enable --name=myservice\n",
        );
    }

    #[test]
    fn test_fix_override_dh_systemd_start() {
        let tmp = TempDir::new().unwrap();
        write_compat(tmp.path(), 13);
        let rules = tmp.path().join("debian/rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_systemd_start:\n\tdh_systemd_start --restart-after-upgrade\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_installsystemd:\n\tdh_systemd_start --restart-after-upgrade\n",
        );
    }

    #[test]
    fn test_no_change_with_compat_level_10() {
        let tmp = TempDir::new().unwrap();
        write_compat(tmp.path(), 10);
        fs::write(
            tmp.path().join("debian/rules"),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_systemd_enable:\n\tdh_systemd_enable\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_deprecated_override() {
        let tmp = TempDir::new().unwrap();
        write_compat(tmp.path(), 12);
        fs::write(
            tmp.path().join("debian/rules"),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_installsystemd:\n\tdh_installsystemd\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
        let tmp = TempDir::new().unwrap();
        write_compat(tmp.path(), 11);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
