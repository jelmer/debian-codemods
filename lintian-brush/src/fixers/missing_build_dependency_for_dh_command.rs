use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_analyzer::relations::ensure_some_version;
use debian_analyzer::rules::dh_invoke_get_with;
use makefile_lossless::Makefile;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const LINTIAN_DATA_PATH: &str = "/usr/share/lintian/data";

#[derive(Debug, Deserialize)]
struct CommandInfo {
    installed_by: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CommandsData {
    commands: HashMap<String, CommandInfo>,
}

#[derive(Debug, Deserialize)]
struct AddOnsData {
    add_ons: HashMap<String, CommandInfo>,
}

fn load_command_deps() -> HashMap<String, String> {
    let mut command_to_dep = HashMap::new();

    // Try loading from lintian data
    let commands_path = format!("{}/debhelper/commands.json", LINTIAN_DATA_PATH);
    if let Ok(content) = fs::read_to_string(&commands_path) {
        if let Ok(data) = serde_json::from_str::<CommandsData>(&content) {
            for (command, info) in data.commands {
                command_to_dep.insert(command, info.installed_by.join(" | "));
            }
        }
    }

    // Add hardcoded mappings (from Python code)
    let hardcoded = [
        ("dh_apache2", "dh-apache2 | apache2-dev"),
        (
            "dh_autoreconf_clean",
            "dh-autoreconf | debhelper (>= 9.20160403~) | debhelper-compat",
        ),
        (
            "dh_autoreconf",
            "dh-autoreconf | debhelper (>= 9.20160403~) | debhelper-compat",
        ),
        ("dh_dkms", "dkms | dh-sequence-dkms"),
        ("dh_girepository", "gobject-introspection | dh-sequence-gir"),
        ("dh_gnome", "gnome-pkg-tools | dh-sequence-gnome"),
        ("dh_gnome_clean", "gnome-pkg-tools | dh-sequence-gnome"),
        ("dh_lv2config", "lv2core"),
        ("dh_make_pgxs", "postgresql-server-dev-all | postgresql-all"),
        ("dh_nativejava", "gcj-native-helper | default-jdk-builddep"),
        ("dh_pgxs_test", "postgresql-server-dev-all | postgresql-all"),
        ("dh_python2", "dh-python | dh-sequence-python2"),
        ("dh_python3", "dh-python | dh-sequence-python3"),
        ("dh_sphinxdoc", "sphinx | python-sphinx | python3-sphinx"),
        ("dh_xine", "libxine-dev | libxine2-dev"),
    ];

    for (cmd, dep) in hardcoded {
        command_to_dep.insert(cmd.to_string(), dep.to_string());
    }

    command_to_dep
}

fn load_addon_deps() -> HashMap<String, String> {
    let mut addon_to_dep = HashMap::new();

    // Try loading from lintian data
    let addons_path = format!("{}/debhelper/add_ons.json", LINTIAN_DATA_PATH);
    if let Ok(content) = fs::read_to_string(&addons_path) {
        if let Ok(data) = serde_json::from_str::<AddOnsData>(&content) {
            for (addon, info) in data.add_ons {
                addon_to_dep.insert(addon, info.installed_by.join(" | "));
            }
        }
    }

    // Add hardcoded mappings (from Python code)
    let hardcoded = [
        ("ada_library", "dh-ada-library | dh-sequence-ada-library"),
        ("apache2", "dh-apache2 | apache2-dev"),
        (
            "autoreconf",
            "dh-autoreconf | debhelper (>= 9.20160403~) | debhelper-compat",
        ),
        ("cli", "cli-common-dev | dh-sequence-cli"),
        ("dwz", "debhelper | debhelper-compat | dh-sequence-dwz"),
        (
            "installinitramfs",
            "debhelper | debhelper-compat | dh-sequence-installinitramfs",
        ),
        ("gnome", "gnome-pkg-tools | dh-sequence-gnome"),
        ("lv2config", "lv2core"),
        ("nodejs", "pkg-js-tools | dh-sequence-nodejs"),
        ("perl_dbi", "libdbi-perl | dh-sequence-perl-dbi"),
        ("perl_imager", "libimager-perl | dh-sequence-perl-imager"),
        ("pgxs", "postgresql-server-dev-all | postgresql-all"),
        ("pgxs_loop", "postgresql-server-dev-all | postgresql-all"),
        ("pypy", "dh-python | dh-sequence-pypy"),
        (
            "python2",
            "python2:any | python2-dev:any | dh-sequence-python2",
        ),
        (
            "python3",
            "python3:any | python3-all:any | python3-dev:any | python3-all-dev:any | dh-sequence-python3",
        ),
        ("scour", "scour | python-scour | dh-sequence-scour"),
        (
            "sphinxdoc",
            "sphinx | python-sphinx | python3-sphinx | dh-sequence-sphinxdoc",
        ),
        (
            "systemd",
            "debhelper (>= 9.20160709~) | debhelper-compat | dh-sequence-systemd | dh-systemd",
        ),
        ("vim_addon", "dh-vim-addon | dh-sequence-vim-addon"),
    ];

    for (addon, dep) in hardcoded {
        addon_to_dep.insert(addon.to_string(), dep.to_string());
    }

    addon_to_dep
}

/// Check if a required dependency is implied by an existing dependency string
fn is_relation_implied(required: &str, existing: &str) -> bool {
    use debian_control::lossless::Relations;

    let (required_relations, _) = Relations::parse_relaxed(required, true);
    let (existing_relations, _) = Relations::parse_relaxed(existing, true);

    // Check if any entry in required is implied by any entry in existing
    for req_entry in required_relations.entries() {
        for exist_entry in existing_relations.entries() {
            if req_entry.is_implied_by(&exist_entry) {
                return true;
            }
        }
    }

    false
}

pub fn run(base_path: &Path, _preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");
    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let command_to_dep = load_command_deps();
    let addon_to_dep = load_addon_deps();

    let content = fs::read_to_string(&rules_path)?;
    let makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    let mut need: Vec<(String, String, String, LintianIssue)> = Vec::new(); // (dep, kind, name, issue)
    let mut overridden_issues = Vec::new();

    for rule in makefile.rules() {
        for recipe in rule.recipes() {
            let trimmed = recipe.trim();
            if trimmed.starts_with('#') {
                continue;
            }

            // Parse the command
            let parts = shell_words::split(trimmed).unwrap_or_default();

            if parts.is_empty() {
                continue;
            }

            let executable = &parts[0];

            // Check if this command needs a dependency
            if let Some(dep) = command_to_dep.get(executable) {
                let issue = LintianIssue::source_with_info(
                    "missing-build-dependency-for-dh_-command",
                    vec![format!(
                        "{} (does not satisfy {}) [debian/rules]",
                        executable, dep
                    )],
                );

                if !issue.should_fix(base_path) {
                    overridden_issues.push(issue);
                } else {
                    need.push((
                        dep.clone(),
                        "command".to_string(),
                        executable.to_string(),
                        issue,
                    ));
                }
            }

            // Check for dh addons
            if executable == "dh" || executable.starts_with("dh_") {
                let addons = dh_invoke_get_with(trimmed);
                for addon in addons {
                    if let Some(dep) = addon_to_dep.get(&addon) {
                        let issue = LintianIssue::source_with_info(
                            "missing-build-dependency-for-dh-addon",
                            vec![format!(
                                "{} (does not satisfy {}) [debian/rules]",
                                addon, dep
                            )],
                        );

                        if !issue.should_fix(base_path) {
                            overridden_issues.push(issue);
                        } else {
                            need.push((dep.clone(), "addon".to_string(), addon, issue));
                        }
                    }
                }
            }
        }
    }

    if need.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut source = editor.source().ok_or(FixerError::NoChanges)?;

    let mut changed: Vec<(String, String, String)> = Vec::new();
    let mut fixed_issues = Vec::new();

    for (dep, kind, name, issue) in need {
        let mut is_implied = false;

        // Check if relation is implied by debhelper
        if is_relation_implied(&dep, "debhelper") {
            is_implied = true;
        }

        // Check all build dependency fields
        if !is_implied {
            for field_name in ["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"] {
                if let Some(field_value) = source.as_deb822().get(field_name) {
                    if is_relation_implied(&dep, &field_value) {
                        is_implied = true;
                        break;
                    }
                }
            }
        }

        if !is_implied {
            let build_depends = source.build_depends().unwrap_or_default();
            let mut new_build_depends = build_depends;
            ensure_some_version(&mut new_build_depends, &dep);
            source.set_build_depends(&new_build_depends);
            changed.push((dep, kind, name));
            fixed_issues.push(issue);
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    let description = if changed.len() == 1 {
        let (dep, kind, name) = &changed[0];
        format!(
            "Add missing build dependency on {} for {} {}",
            dep, kind, name
        )
    } else {
        let mut desc = "Add missing build dependencies:".to_string();
        for (dep, kind, name) in &changed {
            desc.push_str(&format!("\n* {} for {} {}", dep, kind, name));
        }
        desc
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "missing-build-dependency-for-dh_-command",
    tags: ["missing-build-dependency-for-dh_-command", "missing-build-dependency-for-dh-addon"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_no_rules() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_adds_missing_dh_python3() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_content = r#"#!/usr/bin/make

%:
	dh $@

override_dh_build:
	# The next line is empty


	dh_python3
"#;
        fs::write(debian_dir.join("rules"), rules_content).unwrap();

        let control_content = r#"Source: blah
Build-Depends: libc6-dev

Package: python3-blah
Description: blah blah
 blah
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);

        if let Err(ref e) = result {
            eprintln!("Error: {:?}", e);
        }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Add missing build dependency on dh-python | dh-sequence-python3 for command dh_python3"
        );

        // Check that dh-python was added to Build-Depends
        let editor = TemplatedControlEditor::open(&control_path).unwrap();
        let source = editor.source().unwrap();
        let build_depends_str = source.as_deb822().get("Build-Depends").unwrap();
        assert_eq!(
            build_depends_str,
            "dh-python | dh-sequence-python3, libc6-dev"
        );
    }

    #[test]
    fn test_dependency_already_satisfied() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_content = r#"#!/usr/bin/make

%:
	dh $@

override_dh_build:
	dh_python3
"#;
        fs::write(debian_dir.join("rules"), rules_content).unwrap();

        let control_content = r#"Source: blah
Build-Depends: dh-python, libc6-dev

Package: python3-blah
Description: blah blah
 blah
"#;
        fs::write(debian_dir.join("control"), control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
