use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::collections::HashSet;
use std::path::PathBuf;
use strsim::levenshtein;

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

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let known_dh_commands = match get_dh_commands() {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let mut known_targets: HashSet<String> = HashSet::new();
    for cmd in &known_dh_commands {
        known_targets.insert(format!("override_{}", cmd));
        known_targets.insert(format!("execute_before_{}", cmd));
        known_targets.insert(format!("execute_after_{}", cmd));
    }

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut renamed: Vec<(String, String)> = Vec::new();
    for rule in makefile.rules() {
        let line_number = rule.line() + 1;
        for target in rule.targets() {
            let trimmed = target.trim().to_string();
            if known_targets.contains(&trimmed) {
                continue;
            }
            // Match if Levenshtein distance is exactly 1.
            let best_match = known_targets
                .iter()
                .find(|kt| levenshtein(&trimmed, kt) == 1)
                .cloned();
            let Some(best_match) = best_match else {
                continue;
            };
            let issue = LintianIssue::source_with_info(
                "typo-in-debhelper-override-target",
                vec![format!(
                    "{} => {} [debian/rules:{}]",
                    trimmed, best_match, line_number
                )],
            );
            renamed.push((trimmed.clone(), best_match.clone()));
            diagnostics.push(Diagnostic::with_actions(
                issue,
                String::new(),
                vec![Action::Makefile(MakefileAction::RenameRuleTarget {
                    file: rules_rel.clone(),
                    from_target: trimmed,
                    to_target: best_match,
                })],
            ));
        }
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    let summary = format!(
        "Fix typo in debian/rules rules: {}",
        renamed
            .iter()
            .map(|(old, new)| format!("{} \u{21d2} {}", old, new))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for d in &mut diagnostics {
        d.message = summary.clone();
    }
    Ok(diagnostics)
}

fn get_dh_commands() -> Result<Vec<String>, FixerError> {
    const LINTIAN_DATA_PATH: &str = "/usr/share/lintian/data";
    const COMMANDS_JSON_PATH: &str = "/usr/share/lintian/data/debhelper/commands.json";

    let mut dh_commands: Vec<String> = Vec::new();
    if let Ok(content) = std::fs::read_to_string(COMMANDS_JSON_PATH) {
        let data: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| FixerError::Other(format!("Failed to parse commands.json: {}", e)))?;
        if let Some(commands) = data.get("commands").and_then(|c| c.as_object()) {
            dh_commands = commands.keys().cloned().collect();
        }
    } else {
        let dh_commands_path = format!("{}/debhelper/dh_commands", LINTIAN_DATA_PATH);
        let dh_commands_manual_path = format!("{}/debhelper/dh_commands-manual", LINTIAN_DATA_PATH);
        let mut commands_set: HashSet<String> = HashSet::new();
        if let Ok(content) = std::fs::read_to_string(&dh_commands_path) {
            for line in content.lines() {
                if line.starts_with('#') || line.trim().is_empty() {
                    continue;
                }
                if let Some(cmd) = line.split('=').next() {
                    commands_set.insert(cmd.to_string());
                }
            }
        }
        if let Ok(content) = std::fs::read_to_string(&dh_commands_manual_path) {
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
    dh_commands.extend(JAVAHELPER_COMMANDS.iter().map(|s| s.to_string()));
    Ok(dh_commands)
}

declare_detector! {
    name: "typo-in-debhelper-override-target",
    tags: ["typo-in-debhelper-override-target"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_fixes_typo() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\n%:\n\tdh $*\n\noverride_dh_instalman:\n\tinstallman -pfoo\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Fix typo in debian/rules rules: override_dh_instalman \u{21d2} override_dh_installman"
        );
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $*\n\noverride_dh_installman:\n\tinstallman -pfoo\n",
        );
    }

    #[test]
    fn test_no_change_when_no_typo() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make -f\n\n%:\n\tdh $*\n\noverride_dh_installman:\n\tinstallman -pfoo\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_javahelper_commands() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make -f\n\n%:\n\tdh $*\n\noverride_jh_build:\n\tjh_build lala\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
