use crate::diagnostic::{Action, Diagnostic, SystemdAction};
use crate::{FixerError, LintianIssue};
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Map an Alias= value to its corrected form: `.service` is the only
/// extension allowed for an alias of a `.service` unit. Returns the new
/// value, or `None` if the existing value already has the right shape.
fn fix_alias(alias: &str) -> Option<String> {
    const SERVICE_EXT: &str = ".service";
    match alias.rfind('.') {
        Some(idx) => {
            let (base, ext) = (&alias[..idx], &alias[idx..]);
            if ext == SERVICE_EXT {
                None
            } else {
                Some(format!("{}{}", base, SERVICE_EXT))
            }
        }
        None => Some(format!("{}{}", alias, SERVICE_EXT)),
    }
}

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let debian_path = base_path.join("debian");
    if !debian_path.exists() {
        return Ok(Vec::new());
    }

    let mut diagnostics = Vec::new();
    for entry in std::fs::read_dir(&debian_path)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("service") {
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

        let alias_values = unit_section.get_all("Alias");
        if alias_values.is_empty() {
            continue;
        }

        // Pair (old, new) for each alias that needs fixing. If none do,
        // skip the file.
        let fixes: Vec<(String, String)> = alias_values
            .iter()
            .filter_map(|a| fix_alias(a).map(|new| (a.clone(), new)))
            .collect();
        if fixes.is_empty() {
            continue;
        }

        let rel: PathBuf = path.strip_prefix(base_path).unwrap_or(&path).to_path_buf();
        let rel_str = rel.to_string_lossy().to_string();
        let issue = LintianIssue::source_with_info(
            "systemd-service-alias-without-extension",
            vec![rel_str],
        );

        // Emit one (RemoveValue, Add) pair per malformed alias. Aliases
        // that are already correct are left alone, preserving their
        // position in the file.
        let mut actions = Vec::with_capacity(fixes.len() * 2);
        for (old, new) in fixes {
            actions.push(Action::Systemd(SystemdAction::RemoveValue {
                file: rel.clone(),
                section: "Unit".into(),
                field: "Alias".into(),
                value: old,
            }));
            actions.push(Action::Systemd(SystemdAction::Add {
                file: rel.clone(),
                section: "Unit".into(),
                field: "Alias".into(),
                value: new,
            }));
        }

        diagnostics.push(Diagnostic::with_actions(
            issue,
            "Use proper extensions in Alias in systemd files.",
            actions,
        ));
    }

    Ok(diagnostics)
}

declare_fixer! {
    name: "systemd-service-alias-without-extension",
    tags: ["systemd-service-alias-without-extension"],
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
    fn test_fix_alias() {
        assert_eq!(fix_alias("bar"), Some("bar.service".to_string()));
        assert_eq!(fix_alias("bar.target"), Some("bar.service".to_string()));
        assert_eq!(fix_alias("bar.service"), None);
    }

    #[test]
    fn test_add_service_extension_to_alias() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\nAlias=bar\n\n[Service]\nType=oneshot\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test Service\nAlias=bar.service\n\n[Service]\nType=oneshot\n",
        );
    }

    #[test]
    fn test_replace_wrong_extension() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\nAlias=bar.target\n\n[Service]\nType=oneshot\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test Service\nAlias=bar.service\n\n[Service]\nType=oneshot\n",
        );
    }

    #[test]
    fn test_multiple_aliases() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\nAlias=foo\nAlias=bar.target\n\n[Service]\nType=oneshot\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test Service\nAlias=foo.service\nAlias=bar.service\n\n[Service]\nType=oneshot\n",
        );
    }

    #[test]
    fn test_correct_extension_unchanged() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        let original =
            "[Unit]\nDescription=Test Service\nAlias=bar.service\n\n[Service]\nType=oneshot\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_partial_correct_aliases() {
        // Mixed: one already-correct, one needing a fix. Only the broken
        // one should be touched, and the correct one keeps its place.
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("foo.service");
        fs::write(
            &path,
            "[Unit]\nDescription=Test Service\nAlias=foo.service\nAlias=bar\n\n[Service]\nType=oneshot\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=Test Service\nAlias=foo.service\nAlias=bar.service\n\n[Service]\nType=oneshot\n",
        );
    }
}
