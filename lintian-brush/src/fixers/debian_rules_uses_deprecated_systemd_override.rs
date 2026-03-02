use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::debhelper::get_debhelper_compat_level;
use makefile_lossless::Makefile;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");
    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Check debhelper compat level
    let compat_level = get_debhelper_compat_level(base_path)?;
    if let Some(level) = compat_level {
        if level < 11 {
            // This issue only applies to compat level >= 11
            return Err(FixerError::NoChanges);
        }
    } else {
        // Could not determine compat level, bail out
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&rules_path)?;
    let makefile: Makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut renamed_targets = Vec::new();

    // Look for deprecated override targets
    let deprecated_overrides = ["override_dh_systemd_enable", "override_dh_systemd_start"];

    for mut rule in makefile.rules() {
        let targets: Vec<String> = rule.targets().map(|t| t.trim().to_string()).collect();

        for target_str in &targets {
            if deprecated_overrides.contains(&target_str.as_str()) {
                let issue = LintianIssue::source_with_info(
                    "debian-rules-uses-deprecated-systemd-override",
                    vec![target_str.clone()],
                );

                if !issue.should_fix(base_path) {
                    overridden_issues.push(issue);
                    continue;
                }

                // Rename to override_dh_installsystemd
                let new_target = "override_dh_installsystemd";

                // Check if override_dh_installsystemd already exists
                let already_exists = makefile
                    .rules()
                    .any(|r| r.targets().any(|t| t.trim() == new_target));

                if already_exists {
                    // If it already exists, we need to merge or skip
                    // For now, we'll skip this case as it requires manual intervention
                    return Err(FixerError::Other(format!(
                        "Cannot rename {} to {} because {} already exists",
                        target_str, new_target, new_target
                    )));
                }

                rule.rename_target(target_str, new_target).ok();
                renamed_targets.push(target_str.clone());
                fixed_issues.push(issue);
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Write back the modified makefile
    fs::write(&rules_path, makefile.to_string())?;

    let description = if renamed_targets.len() == 1 {
        format!(
            "Replace deprecated {} with override_dh_installsystemd",
            renamed_targets[0]
        )
    } else {
        format!(
            "Replace deprecated systemd overrides ({}) with override_dh_installsystemd",
            renamed_targets.join(", ")
        )
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "debian-rules-uses-deprecated-systemd-override",
    tags: ["debian-rules-uses-deprecated-systemd-override"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_fix_override_dh_systemd_enable() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(debian_dir.join("compat"), "11\n").unwrap();

        let rules_content = r#"#!/usr/bin/make -f

%:
	dh $@

override_dh_systemd_enable:
	dh_systemd_enable --name=myservice
"#;
        fs::write(debian_dir.join("rules"), rules_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(result.is_ok(), "Error: {:?}", result);
        let result = result.unwrap();
        assert!(result.description.contains("override_dh_systemd_enable"));
        assert!(result.description.contains("override_dh_installsystemd"));

        let updated_content = fs::read_to_string(debian_dir.join("rules")).unwrap();
        assert!(updated_content.contains("override_dh_installsystemd:"));
        assert!(!updated_content.contains("override_dh_systemd_enable:"));
    }

    #[test]
    fn test_fix_override_dh_systemd_start() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(debian_dir.join("compat"), "13\n").unwrap();

        let rules_content = r#"#!/usr/bin/make -f

%:
	dh $@

override_dh_systemd_start:
	dh_systemd_start --restart-after-upgrade
"#;
        fs::write(debian_dir.join("rules"), rules_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(result.is_ok());

        let updated_content = fs::read_to_string(debian_dir.join("rules")).unwrap();
        assert!(updated_content.contains("override_dh_installsystemd:"));
        assert!(!updated_content.contains("override_dh_systemd_start:"));
    }

    #[test]
    fn test_no_change_with_compat_level_10() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(debian_dir.join("compat"), "10\n").unwrap();

        let rules_content = r#"#!/usr/bin/make -f

%:
	dh $@

override_dh_systemd_enable:
	dh_systemd_enable --name=myservice
"#;
        fs::write(debian_dir.join("rules"), rules_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        // Should not change because compat level < 11
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_deprecated_override() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(debian_dir.join("compat"), "12\n").unwrap();

        let rules_content = r#"#!/usr/bin/make -f

%:
	dh $@

override_dh_installsystemd:
	dh_installsystemd --name=myservice
"#;
        fs::write(debian_dir.join("rules"), rules_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(debian_dir.join("compat"), "11\n").unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
