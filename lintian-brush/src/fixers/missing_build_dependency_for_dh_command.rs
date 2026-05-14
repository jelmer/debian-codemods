use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_analyzer::rules::dh_invoke_get_with;
use debian_workspace::Workspace;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

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
    if let Ok(content) = std::fs::read_to_string(&commands_path) {
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
    if let Ok(content) = std::fs::read_to_string(&addons_path) {
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

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };

    let command_to_dep = load_command_deps();
    let addon_to_dep = load_addon_deps();

    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let existing_bd: Vec<String> = ["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"]
        .iter()
        .filter_map(|f| source.as_deb822().get(f))
        .collect();

    let is_already_satisfied = |dep: &str| -> bool {
        if is_relation_implied(dep, "debhelper") {
            return true;
        }
        existing_bd.iter().any(|v| is_relation_implied(dep, v))
    };

    // (dep, kind, name, issue)
    let mut need: Vec<(String, &'static str, String, LintianIssue)> = Vec::new();
    for rule in makefile.rules() {
        for recipe in rule.recipes() {
            let trimmed = recipe.trim();
            if trimmed.starts_with('#') {
                continue;
            }
            let parts = shell_words::split(trimmed).unwrap_or_default();
            let Some(executable) = parts.first().cloned() else {
                continue;
            };

            if let Some(dep) = command_to_dep.get(&executable) {
                let issue = LintianIssue::source_with_info(
                    "missing-build-dependency-for-dh_-command",
                    Visibility::Error,
                    vec![format!(
                        "{} (does not satisfy {}) [debian/rules]",
                        executable, dep
                    )],
                );
                need.push((dep.clone(), "command", executable.clone(), issue));
            }

            if executable == "dh" || executable.starts_with("dh_") {
                let addons = dh_invoke_get_with(trimmed);
                for addon in addons {
                    if let Some(dep) = addon_to_dep.get(&addon) {
                        let issue = LintianIssue::source_with_info(
                            "missing-build-dependency-for-dh-addon",
                            Visibility::Error,
                            vec![format!(
                                "{} (does not satisfy {}) [debian/rules]",
                                addon, dep
                            )],
                        );
                        need.push((dep.clone(), "addon", addon, issue));
                    }
                }
            }
        }
    }

    let mut effective: Vec<(String, &'static str, String, LintianIssue)> = Vec::new();
    let mut emitted_deps = std::collections::HashSet::<String>::new();
    for entry in need {
        if is_already_satisfied(&entry.0) {
            continue;
        }
        if !emitted_deps.insert(entry.0.clone()) {
            continue;
        }
        effective.push(entry);
    }

    if effective.is_empty() {
        return Ok(Vec::new());
    }

    // Each diagnostic carries its own EnsureRelation. The applier runs
    // them in order against the same Build-Depends field; ensure_relation
    // is idempotent and additive so concurrent dependent edits compose
    // correctly.
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for (dep, kind, name, issue) in effective {
        let description = format!(
            "Build dependency on {} is missing for {} {}.",
            dep, kind, name
        );
        let label = format!(
            "Add missing build dependency on {} for {} {}.",
            dep, kind, name
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            description,
            label,
            vec![Action::Deb822(Deb822Action::EnsureRelation {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: "Build-Depends".into(),
                entry: dep,
            })],
        ));
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "missing-build-dependency-for-dh_-command",
    tags: ["missing-build-dependency-for-dh_-command", "missing-build-dependency-for-dh-addon"],
    triggers: [
        debian_workspace::Trigger::File("debian/rules"),
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends-Indep",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends-Arch",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
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
    fn test_no_rules() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_adds_missing_dh_python3() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make\n\n%:\n\tdh $@\n\noverride_dh_build:\n\t# The next line is empty\n\n\n\tdh_python3\n",
        )
        .unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: blah\nBuild-Depends: libc6-dev\n\nPackage: python3-blah\nDescription: blah blah\n blah\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Add missing build dependency on dh-python | dh-sequence-python3 for command dh_python3."
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\nBuild-Depends: dh-python | dh-sequence-python3, libc6-dev\n\nPackage: python3-blah\nDescription: blah blah\n blah\n",
        );
    }

    #[test]
    fn test_dependency_already_satisfied() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make\n\n%:\n\tdh $@\n\noverride_dh_build:\n\tdh_python3\n",
        )
        .unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: dh-python, libc6-dev\n\nPackage: python3-blah\nDescription: blah blah\n blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
