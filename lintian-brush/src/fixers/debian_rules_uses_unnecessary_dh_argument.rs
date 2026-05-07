use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::workspace::{compat_level, FixerWorkspace};
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_analyzer::rules::{dh_invoke_drop_argument, dh_invoke_drop_with};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");

    let compat_version = compat_level(ws)?;
    let mut unnecessary_args: Vec<&str> = Vec::new();
    let mut unnecessary_with: Vec<&str> = Vec::new();
    if let Some(compat) = compat_version {
        if compat >= 10 {
            unnecessary_args.push("--parallel");
            unnecessary_with.push("systemd");
        }
    }
    if unnecessary_args.is_empty() && unnecessary_with.is_empty() {
        return Ok(Vec::new());
    }

    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    // First pass: scan wildcard rules for `--no-X` to skip the matching `--X`
    // (and vice versa) since they cancel each other out.
    let mut args_to_skip: Vec<String> = Vec::new();
    for rule in makefile.rules() {
        if !rule.targets().any(|t| t.contains('%')) {
            continue;
        }
        for recipe in rule.recipes() {
            if !recipe.trim().starts_with("dh ") {
                continue;
            }
            for arg in &unnecessary_args {
                if let Some(stripped) = arg.strip_prefix("--") {
                    let negative = format!("--no-{}", stripped);
                    if recipe.contains(&negative) {
                        args_to_skip.push(arg.to_string());
                    }
                }
                if let Some(stripped) = arg.strip_prefix("--no-") {
                    let positive = format!("--{}", stripped);
                    if recipe.contains(&positive) {
                        args_to_skip.push(arg.to_string());
                    }
                }
            }
        }
    }
    unnecessary_args.retain(|a| !args_to_skip.contains(&a.to_string()));

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut removed_args: Vec<String> = Vec::new();

    for rule in makefile.rules() {
        let Some(target) = rule.targets().next() else {
            continue;
        };
        for recipe_node in rule.recipe_nodes() {
            let recipe = recipe_node.text();
            let line_no = recipe_node.line() + 1;
            let mut modified = recipe.to_string();
            let mut recipe_changed = false;
            let mut issues: Vec<LintianIssue> = Vec::new();

            for arg in &unnecessary_args {
                if !modified.contains(arg) {
                    continue;
                }
                let info = if let Some(compat) = compat_version {
                    format!("{} >= 10 dh ... {} [debian/rules:{}]", compat, arg, line_no)
                } else {
                    format!("dh ... {} [debian/rules:{}]", arg, line_no)
                };
                let new_recipe = dh_invoke_drop_argument(&modified, arg);
                if new_recipe != modified {
                    modified = new_recipe;
                    recipe_changed = true;
                    if !removed_args.contains(&arg.to_string()) {
                        removed_args.push(arg.to_string());
                    }
                    issues.push(LintianIssue::source_with_info(
                        "debian-rules-uses-unnecessary-dh-argument",
                        vec![info],
                    ));
                }
            }

            for with_val in &unnecessary_with {
                let with_arg = format!("--with={}", with_val);
                let with_space = format!("--with {}", with_val);
                if !modified.contains(&with_arg) && !modified.contains(&with_space) {
                    continue;
                }
                let info = if let Some(compat) = compat_version {
                    format!(
                        "{} >= 10 dh ... {} [debian/rules:{}]",
                        compat, with_arg, line_no
                    )
                } else {
                    format!("dh ... {} [debian/rules:{}]", with_arg, line_no)
                };
                let new_recipe = dh_invoke_drop_with(&modified, with_val);
                if new_recipe != modified {
                    modified = new_recipe;
                    recipe_changed = true;
                    if !removed_args.contains(&with_arg) {
                        removed_args.push(with_arg.clone());
                    }
                    issues.push(LintianIssue::source_with_info(
                        "debian-rules-uses-unnecessary-dh-argument",
                        vec![info],
                    ));
                }
            }

            if !recipe_changed {
                continue;
            }
            let action = Action::Makefile(MakefileAction::ReplaceRecipe {
                file: rules_rel.clone(),
                target: target.clone(),
                recipe: recipe.to_string(),
                new_recipe: modified,
            });
            for (idx, issue) in issues.into_iter().enumerate() {
                let actions = if idx == 0 {
                    vec![action.clone()]
                } else {
                    Vec::new()
                };
                diagnostics.push(Diagnostic::with_actions(
                    issue,
                    "debian/rules uses unnecessary dh arguments.",
                    "Drop unnecessary dh arguments.",
                    actions,
                ));
            }
        }
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    let summary = format!("Drop unnecessary dh arguments: {}", removed_args.join(", "));
    for d in &mut diagnostics {
        for plan in &mut d.plans {
            plan.label = summary.clone();
        }
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "debian-rules-uses-unnecessary-dh-argument",
    tags: ["debian-rules-uses-unnecessary-dh-argument"],
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
    fn test_drop_parallel_at_compat_10() {
        let tmp = TempDir::new().unwrap();
        write_compat(tmp.path(), 10);
        let rules = tmp.path().join("debian/rules");
        fs::write(&rules, "%:\n\tdh $@ --parallel\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(fs::read_to_string(&rules).unwrap(), "%:\n\tdh $@\n",);
    }

    #[test]
    fn test_drop_with_systemd_at_compat_10() {
        let tmp = TempDir::new().unwrap();
        write_compat(tmp.path(), 10);
        let rules = tmp.path().join("debian/rules");
        fs::write(&rules, "%:\n\tdh $@ --with=systemd\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(fs::read_to_string(&rules).unwrap(), "%:\n\tdh $@\n",);
    }

    #[test]
    fn test_no_change_when_no_parallel() {
        let tmp = TempDir::new().unwrap();
        write_compat(tmp.path(), 10);
        let rules = tmp.path().join("debian/rules");
        fs::write(&rules, "%:\n\tdh $@\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_parallel_at_compat_9() {
        let tmp = TempDir::new().unwrap();
        write_compat(tmp.path(), 9);
        let rules = tmp.path().join("debian/rules");
        fs::write(&rules, "%:\n\tdh $@ --parallel\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_skip_parallel_when_no_parallel_in_wildcard() {
        let tmp = TempDir::new().unwrap();
        write_compat(tmp.path(), 10);
        let rules = tmp.path().join("debian/rules");
        // The `--no-parallel` in the wildcard rule signals an explicit
        // override of the default — don't strip `--parallel` elsewhere.
        fs::write(
            &rules,
            "%:\n\tdh $@ --no-parallel\n\nbuild:\n\tdh $@ --parallel\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
