use crate::rules::drop_dh_with_argument;
use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_analyzer::relations::ensure_minimum_version;
use debversion::Version;
use makefile_lossless::Makefile;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");
    let control_path = base_path.join("debian/control");

    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Read and parse the makefile
    let content = fs::read_to_string(&rules_path)?;
    let makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    let mut made_changes = false;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Process all rules and their commands
    for mut rule in makefile.rules() {
        let mut commands_to_update = Vec::new();
        let mut commands_to_remove = Vec::new();

        for (i, recipe_node) in rule.recipe_nodes().enumerate() {
            let recipe = recipe_node.text();
            let trimmed = recipe.trim();
            let line_no = recipe_node.line() + 1;

            // Replace dh_autotools-dev_updateconfig with dh_update_autotools_config
            if trimmed == "dh_autotools-dev_updateconfig" {
                let issue = LintianIssue::source_with_info(
                    "debhelper-tools-from-autotools-dev-are-deprecated",
                    vec![format!(
                        "dh_autotools-dev_updateconfig [debian/rules:{}]",
                        line_no
                    )],
                );

                if issue.should_fix(base_path) {
                    // Preserve original indentation
                    let original = recipe.to_string();
                    let indent: String =
                        original.chars().take_while(|c| c.is_whitespace()).collect();
                    commands_to_update.push((i, format!("{}dh_update_autotools_config", indent)));
                    made_changes = true;
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
                continue;
            }

            // Remove dh_autotools-dev_restoreconfig
            if trimmed == "dh_autotools-dev_restoreconfig" {
                let issue = LintianIssue::source_with_info(
                    "debhelper-tools-from-autotools-dev-are-deprecated",
                    vec![format!(
                        "dh_autotools-dev_restoreconfig [debian/rules:{}]",
                        line_no
                    )],
                );

                if issue.should_fix(base_path) {
                    commands_to_remove.push(i);
                    made_changes = true;
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
                continue;
            }

            // Drop --with autotools-dev and --with autotools_dev from dh invocations
            let mut new_recipe = recipe.to_string();
            let original_recipe = new_recipe.clone();

            let with_autotools_dev = drop_dh_with_argument(&new_recipe, "autotools-dev");
            if with_autotools_dev != new_recipe {
                new_recipe = with_autotools_dev;
            }

            let with_autotools_underscore = drop_dh_with_argument(&new_recipe, "autotools_dev");
            if with_autotools_underscore != new_recipe {
                new_recipe = with_autotools_underscore;
            }

            if new_recipe != original_recipe {
                let issue = LintianIssue::source_with_info(
                    "debhelper-tools-from-autotools-dev-are-deprecated",
                    vec![format!(
                        "dh ... --with autotools_dev [debian/rules:{}]",
                        line_no
                    )],
                );

                if issue.should_fix(base_path) {
                    commands_to_update.push((i, new_recipe));
                    made_changes = true;
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
            }
        }

        // Apply updates (do removals after updates to maintain indices)
        for (i, new_recipe) in commands_to_update {
            if !commands_to_remove.contains(&i) {
                rule.replace_command(i, &new_recipe);
            }
        }

        // Remove commands (in reverse order to maintain indices)
        for i in commands_to_remove.iter().rev() {
            rule.remove_command(*i);
        }
    }

    if !made_changes {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Write the updated rules file back
    fs::write(&rules_path, makefile.to_string())?;

    // Ensure minimum debhelper version (only if using debhelper directly, not debhelper-compat)
    if control_path.exists() {
        let editor = TemplatedControlEditor::open(&control_path)?;
        if let Some(mut source) = editor.source() {
            let build_depends = source.build_depends().unwrap_or_default();

            // Check if debhelper-compat is present
            let has_debhelper_compat = build_depends
                .entries()
                .any(|entry| entry.relations().any(|r| r.name() == "debhelper-compat"));

            // Only update debhelper version if debhelper-compat is not present
            if !has_debhelper_compat {
                let mut build_depends = build_depends;
                let min_version: Version = "9.20160114".parse().unwrap();

                if ensure_minimum_version(&mut build_depends, "debhelper", &min_version) {
                    // Update the paragraph directly to preserve formatting
                    let paragraph = source.as_mut_deb822();
                    paragraph.set("Build-Depends", &build_depends.to_string());
                    editor.commit()?;
                }
            }
        }
    }

    Ok(FixerResult::builder("Drop use of autotools-dev debhelper.")
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "debhelper-tools-from-autotools-dev-are-deprecated",
    tags: ["debhelper-tools-from-autotools-dev-are-deprecated"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_replace_updateconfig() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(
            &rules_path,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_auto_configure:\n\tdh_autotools-dev_updateconfig\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Drop use of autotools-dev debhelper.");

        let content = fs::read_to_string(&rules_path).unwrap();
        assert!(content.contains("dh_update_autotools_config"));
        assert!(!content.contains("dh_autotools-dev_updateconfig"));
    }

    #[test]
    fn test_remove_restoreconfig() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(
            &rules_path,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_auto_clean:\n\tdh_autotools-dev_restoreconfig\n\tdh_auto_clean\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Drop use of autotools-dev debhelper.");

        let content = fs::read_to_string(&rules_path).unwrap();
        assert!(!content.contains("dh_autotools-dev_restoreconfig"));
        assert!(content.contains("dh_auto_clean"));
    }

    #[test]
    fn test_drop_with_autotools_dev() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(
            &rules_path,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with autotools-dev\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Drop use of autotools-dev debhelper.");

        let content = fs::read_to_string(&rules_path).unwrap();
        assert!(!content.contains("--with autotools-dev"));
        assert!(content.contains("dh $@"));
    }

    #[test]
    fn test_drop_with_autotools_underscore() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(
            &rules_path,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with autotools_dev\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Drop use of autotools-dev debhelper.");

        let content = fs::read_to_string(&rules_path).unwrap();
        assert!(!content.contains("--with autotools_dev"));
        assert!(content.contains("dh $@"));
    }

    #[test]
    fn test_no_autotools_dev() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, "#!/usr/bin/make -f\n\n%:\n\tdh $@\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
