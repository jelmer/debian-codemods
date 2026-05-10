use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, MakefileAction, ParagraphSelector};
use crate::rules::drop_dh_with_argument;
use debian_workspace::Workspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::PathBuf;

const MIN_DEBHELPER_VERSION: &str = "9.20160114";

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut issues: Vec<LintianIssue> = Vec::new();
    let mut actions: Vec<Action> = Vec::new();

    for rule in makefile.rules() {
        let Some(target) = rule.targets().next() else {
            continue;
        };
        for recipe_node in rule.recipe_nodes() {
            let recipe = recipe_node.text();
            let trimmed = recipe.trim();
            let line_no = recipe_node.line() + 1;

            if trimmed == "dh_autotools-dev_updateconfig" {
                let indent: String = recipe.chars().take_while(|c| c.is_whitespace()).collect();
                issues.push(LintianIssue::source_with_info(
                    "debhelper-tools-from-autotools-dev-are-deprecated",
                    Visibility::Warning,
                    vec![format!(
                        "dh_autotools-dev_updateconfig [debian/rules:{}]",
                        line_no
                    )],
                ));
                actions.push(Action::Makefile(MakefileAction::ReplaceRecipe {
                    file: rules_rel.clone(),
                    target: target.clone(),
                    recipe: recipe.to_string(),
                    new_recipe: format!("{}dh_update_autotools_config", indent),
                }));
                continue;
            }

            if trimmed == "dh_autotools-dev_restoreconfig" {
                issues.push(LintianIssue::source_with_info(
                    "debhelper-tools-from-autotools-dev-are-deprecated",
                    Visibility::Warning,
                    vec![format!(
                        "dh_autotools-dev_restoreconfig [debian/rules:{}]",
                        line_no
                    )],
                ));
                actions.push(Action::Makefile(MakefileAction::RemoveRecipe {
                    file: rules_rel.clone(),
                    target: target.clone(),
                    recipe: recipe.to_string(),
                }));
                continue;
            }

            // Drop --with autotools-dev / autotools_dev from dh invocations.
            let stripped_dash = drop_dh_with_argument(&recipe, "autotools-dev");
            let stripped = drop_dh_with_argument(&stripped_dash, "autotools_dev");
            if stripped != recipe {
                issues.push(LintianIssue::source_with_info(
                    "debhelper-tools-from-autotools-dev-are-deprecated",
                    Visibility::Warning,
                    vec![format!(
                        "dh ... --with autotools_dev [debian/rules:{}]",
                        line_no
                    )],
                ));
                actions.push(Action::Makefile(MakefileAction::ReplaceRecipe {
                    file: rules_rel.clone(),
                    target: target.clone(),
                    recipe: recipe.to_string(),
                    new_recipe: stripped,
                }));
            }
        }
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    // Bump debhelper minimum if we're using debhelper directly (no
    // debhelper-compat seen). The applier's EnsureRelation handles >=.
    let control_rel = PathBuf::from("debian/control");
    if let Ok(control) = ws.parsed_control() {
        if let Some(source) = control.source() {
            let bd_str = source.as_deb822().get("Build-Depends").unwrap_or_default();
            let has_compat = bd_str.contains("debhelper-compat");
            if !has_compat {
                actions.push(Action::Deb822(Deb822Action::EnsureRelation {
                    file: control_rel,
                    paragraph: ParagraphSelector::Source,
                    field: "Build-Depends".into(),
                    entry: format!("debhelper (>= {})", MIN_DEBHELPER_VERSION),
                }));
            }
        }
    }

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for (i, issue) in issues.into_iter().enumerate() {
        let plan_actions = if i == 0 { actions.clone() } else { Vec::new() };
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "Deprecated debhelper tools from autotools-dev are used.",
            "Drop use of autotools-dev debhelper.",
            plan_actions,
        ));
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "debhelper-tools-from-autotools-dev-are-deprecated",
    tags: ["debhelper-tools-from-autotools-dev-are-deprecated"],
    triggers: [
        debian_workspace::Trigger::File("debian/rules"),
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
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
    fn test_replace_updateconfig() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_auto_configure:\n\tdh_autotools-dev_updateconfig\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, "Drop use of autotools-dev debhelper.");
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_auto_configure:\n\tdh_update_autotools_config\n",
        );
    }

    #[test]
    fn test_remove_restoreconfig() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_auto_clean:\n\tdh_autotools-dev_restoreconfig\n\tdh_auto_clean\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_auto_clean:\n\tdh_auto_clean\n",
        );
    }

    #[test]
    fn test_drop_with_autotools_dev() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with autotools-dev\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_drop_with_autotools_underscore() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with autotools_dev\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_no_autotools_dev() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("rules"), "#!/usr/bin/make -f\n\n%:\n\tdh $@\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
