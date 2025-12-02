use crate::{declare_fixer, FixerError, FixerResult};
use makefile_lossless::Makefile;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use strsim::levenshtein;

/// Include javahelper binaries, since some are just one character away from
/// debhelper ones.
const JAVAHELPER_COMMANDS: &[&str] = &[
    "jh_build",
    "jh_classpath",
    "jh_clean",
    "jh_compilefeatures",
    "jh_depends",
    "jh_exec",
    "jh_generateorbitdir",
    "jh_installeclipse",
    "jh_installjavadoc",
    "jh_installlibs",
    "jh_linkjars",
    "jh_makepkg",
    "jh_manifest",
    "jh_repack",
    "jh_setupenvironment",
    "mh_checkrepo",
    "mh_install",
    "mh_installpoms",
    "mh_linkjars",
    "mh_patchpoms",
    "mh_clean",
    "mh_installjar",
    "mh_installsite",
    "mh_linkrepojar",
    "mh_unpatchpoms",
    "mh_cleanpom",
    "mh_installpom",
    "mh_linkjar",
    "mh_patchpom",
];

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");
    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Get known dh commands
    let known_dh_commands = get_dh_commands()?;

    // Build set of known targets
    let mut known_targets = HashSet::new();
    for dh_command in &known_dh_commands {
        known_targets.insert(format!("override_{}", dh_command));
        known_targets.insert(format!("execute_before_{}", dh_command));
        known_targets.insert(format!("execute_after_{}", dh_command));
    }

    // Parse the makefile
    let content = fs::read_to_string(&rules_path)?;
    let makefile: Makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    let mut renamed = Vec::new();

    // Check all rules for typos
    for mut rule in makefile.rules() {
        // Collect targets first to avoid borrow checker issues
        let targets: Vec<String> = rule.targets().map(|t| t.trim().to_string()).collect();

        for target_str in targets {
            // Skip if already a known target
            if known_targets.contains(&target_str) {
                continue;
            }

            // Find matching target with Levenshtein distance of 1
            for known_target in &known_targets {
                if levenshtein(&target_str, known_target) == 1 {
                    renamed.push((target_str.to_string(), known_target.clone()));
                    rule.rename_target(&target_str, known_target).ok();
                    break;
                }
            }
        }
    }

    if renamed.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Write back the modified makefile
    fs::write(&rules_path, makefile.to_string())?;

    let description = format!(
        "Fix typo in debian/rules rules: {}",
        renamed
            .iter()
            .map(|(old, new)| format!("{} ⇒ {}", old, new))
            .collect::<Vec<_>>()
            .join(", ")
    );

    Ok(FixerResult::builder(&description)
        .fixed_tags(vec!["typo-in-debhelper-override-target"])
        .build())
}

/// Get list of known dh commands from lintian data files
fn get_dh_commands() -> Result<Vec<String>, FixerError> {
    const LINTIAN_DATA_PATH: &str = "/usr/share/lintian/data";
    const COMMANDS_JSON_PATH: &str = "/usr/share/lintian/data/debhelper/commands.json";

    let mut dh_commands = Vec::new();

    // Try to load from commands.json (newer lintian versions)
    if let Ok(content) = fs::read_to_string(COMMANDS_JSON_PATH) {
        let data: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| FixerError::Other(format!("Failed to parse commands.json: {}", e)))?;

        if let Some(commands) = data.get("commands").and_then(|c| c.as_object()) {
            dh_commands = commands.keys().cloned().collect();
        }
    } else {
        // Fallback: try older lintian data file format
        let dh_commands_path = format!("{}/debhelper/dh_commands", LINTIAN_DATA_PATH);
        let dh_commands_manual_path = format!("{}/debhelper/dh_commands-manual", LINTIAN_DATA_PATH);

        let mut commands_set = HashSet::new();

        if let Ok(content) = fs::read_to_string(&dh_commands_path) {
            for line in content.lines() {
                if line.starts_with('#') || line.trim().is_empty() {
                    continue;
                }
                if let Some(cmd) = line.split('=').next() {
                    commands_set.insert(cmd.to_string());
                }
            }
        }

        if let Ok(content) = fs::read_to_string(&dh_commands_manual_path) {
            for line in content.lines() {
                if line.starts_with('#') || line.trim().is_empty() {
                    continue;
                }
                if let Some(cmd) = line.split("||").next() {
                    commands_set.insert(cmd.to_string());
                }
            }
        }

        if commands_set.is_empty() {
            return Err(FixerError::Other(
                "Could not load dh commands from lintian data".to_string(),
            ));
        }

        dh_commands = commands_set.into_iter().collect();
    }

    // Add javahelper commands
    dh_commands.extend(JAVAHELPER_COMMANDS.iter().map(|s| s.to_string()));

    Ok(dh_commands)
}

declare_fixer! {
    name: "typo-in-debhelper-override-target",
    tags: ["typo-in-debhelper-override-target"],
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
    fn test_fixes_typo() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content =
            "#!/usr/bin/make -f\n\n%:\n\tdh $*\n\noverride_dh_instalman:\n\tinstallman -pfoo\n";
        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.description.contains("override_dh_instalman"));
        assert!(result.description.contains("override_dh_installman"));

        let updated_content = fs::read_to_string(&rules_path).unwrap();
        assert_eq!(
            updated_content,
            "#!/usr/bin/make -f\n\n%:\n\tdh $*\n\noverride_dh_installman:\n\tinstallman -pfoo\n"
        );
    }

    #[test]
    fn test_no_change_when_no_typo() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content =
            "#!/usr/bin/make -f\n\n%:\n\tdh $*\n\noverride_dh_installman:\n\tinstallman -pfoo\n";
        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

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
    fn test_javahelper_commands() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content =
            "#!/usr/bin/make -f\n\n%:\n\tdh $*\n\noverride_jh_build:\n\tjh_build lala\n";
        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        // Should not change as jh_build is a known command
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
        let temp_dir = TempDir::new().unwrap();
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
