use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::debhelper::get_debhelper_compat_level;
use debian_analyzer::rules::{dh_invoke_drop_argument, dh_invoke_drop_with};
use makefile_lossless::Makefile;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");

    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Get debhelper compat level
    let compat_version = get_debhelper_compat_level(base_path)?;

    let mut unnecessary_args = Vec::new();
    let mut unnecessary_with = Vec::new();

    // For compat >= 10, --parallel and --with=systemd are unnecessary
    if let Some(compat) = compat_version {
        if compat >= 10 {
            unnecessary_args.push("--parallel");
            unnecessary_with.push("systemd");
        }
    }

    if unnecessary_args.is_empty() && unnecessary_with.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&rules_path)?;
    let makefile: Makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    // First pass: scan for --no-* arguments to exclude their positive counterparts
    // If a rule contains --no-parallel, we shouldn't remove --parallel
    // If a rule contains --parallel, we shouldn't remove --no-parallel
    let mut args_to_skip = Vec::new();
    for rule in makefile.rules() {
        // Only check rules with % target (wildcard rules like %:)
        if !rule.targets().any(|t| t.contains('%')) {
            continue;
        }

        for recipe in rule.recipes() {
            let recipe_str = recipe.to_string();
            if !recipe_str.trim().starts_with("dh ") {
                continue;
            }

            // Check for --no-* versions of our unnecessary args
            for arg in &unnecessary_args {
                if let Some(stripped) = arg.strip_prefix("--") {
                    let negative = format!("--no-{}", stripped);
                    // If the recipe contains the negative form, skip removing the positive
                    if recipe_str.contains(&negative) {
                        args_to_skip.push(arg.to_string());
                    }
                }
            }

            // Check for positive versions when we have --no-* in unnecessary_args
            for arg in &unnecessary_args {
                if let Some(stripped) = arg.strip_prefix("--no-") {
                    let positive = format!("--{}", stripped);
                    // If the recipe contains the positive form, skip removing the negative
                    if recipe_str.contains(&positive) {
                        args_to_skip.push(arg.to_string());
                    }
                }
            }
        }
    }

    // Remove args that should be skipped
    unnecessary_args.retain(|arg| !args_to_skip.contains(&arg.to_string()));

    let mut made_changes = false;
    let mut removed_args: Vec<String> = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Iterate through rules and modify them
    let mut rules: Vec<_> = makefile.rules().collect();
    for rule in &mut rules {
        for (recipe_index, recipe_node) in rule.recipe_nodes().enumerate() {
            let recipe = recipe_node.text();
            let line_no = recipe_node.line() + 1;
            let mut modified_recipe = recipe.to_string();
            let mut recipe_changed = false;

            // Try to remove each unnecessary argument
            for arg in &unnecessary_args {
                if modified_recipe.contains(arg) {
                    let info = if let Some(compat) = compat_version {
                        format!("{} >= 10 dh ... {} [debian/rules:{}]", compat, arg, line_no)
                    } else {
                        format!("dh ... {} [debian/rules:{}]", arg, line_no)
                    };
                    let issue = crate::LintianIssue::source_with_info(
                        "debian-rules-uses-unnecessary-dh-argument",
                        vec![info],
                    );

                    if issue.should_fix(base_path) {
                        let new_recipe = dh_invoke_drop_argument(&modified_recipe, arg);
                        if new_recipe != modified_recipe {
                            modified_recipe = new_recipe;
                            recipe_changed = true;
                            if !removed_args.contains(&arg.to_string()) {
                                removed_args.push(arg.to_string());
                            }
                            fixed_issues.push(issue);
                        }
                    } else {
                        overridden_issues.push(issue);
                    }
                }
            }

            // Try to remove each unnecessary --with value
            for with_val in &unnecessary_with {
                let with_arg = format!("--with={}", with_val);
                if modified_recipe.contains(&with_arg)
                    || modified_recipe.contains(&format!("--with {}", with_val))
                {
                    let info = if let Some(compat) = compat_version {
                        format!(
                            "{} >= 10 dh ... {} [debian/rules:{}]",
                            compat, with_arg, line_no
                        )
                    } else {
                        format!("dh ... {} [debian/rules:{}]", with_arg, line_no)
                    };
                    let issue = crate::LintianIssue::source_with_info(
                        "debian-rules-uses-unnecessary-dh-argument",
                        vec![info],
                    );

                    if issue.should_fix(base_path) {
                        let new_recipe = dh_invoke_drop_with(&modified_recipe, with_val);
                        if new_recipe != modified_recipe {
                            modified_recipe = new_recipe;
                            recipe_changed = true;
                            if !removed_args.contains(&with_arg) {
                                removed_args.push(with_arg.clone());
                            }
                            fixed_issues.push(issue);
                        }
                    } else {
                        overridden_issues.push(issue);
                    }
                }
            }

            if recipe_changed {
                rule.replace_command(recipe_index, &modified_recipe);
                made_changes = true;
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    // Write back the modified makefile
    fs::write(&rules_path, makefile.to_string())?;

    Ok(FixerResult::builder(format!(
        "Drop unnecessary dh arguments: {}",
        removed_args.join(", ")
    ))
    .fixed_issues(fixed_issues)
    .overridden_issues(overridden_issues)
    .build())
}

declare_fixer! {
    name: "debian-rules-uses-unnecessary-dh-argument",
    tags: ["debian-rules-uses-unnecessary-dh-argument"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}
