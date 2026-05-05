use crate::diagnostic::{Action, Deb822Action, Diagnostic, MakefileAction, ParagraphSelector};
use crate::{FixerError, LintianIssue, PackageType};
use debian_analyzer::rules::dh_invoke_add_with;
use debian_control::lossless::Control;
use makefile_lossless::Makefile;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_abs = base_path.join(&control_rel);
    if !control_abs.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&control_abs)?;
    let Ok(control) = Control::from_str(&content) else {
        return Ok(Vec::new());
    };

    let mut drop_actions: Vec<Action> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for binary in control.binaries() {
        let Some(name) = binary.name() else {
            continue;
        };
        let depends_str = binary.as_deb822().get("Depends").unwrap_or_default();
        if !depends_str.contains("vim-addon-manager") {
            continue;
        }
        let issue = LintianIssue {
            package: Some(name.clone()),
            package_type: Some(PackageType::Binary),
            tag: Some("obsolete-vim-addon-manager".to_string()),
            info: None,
        };
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "Migrate from vim-addon-manager to dh-vim-addon.",
            Vec::new(),
        ));
        drop_actions.push(Action::Deb822(Deb822Action::DropRelation {
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Binary { package: name },
            field: "Depends".into(),
            package: "vim-addon-manager".into(),
        }));
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    let mut all_actions: Vec<Action> = drop_actions;
    all_actions.push(Action::Deb822(Deb822Action::EnsureRelation {
        file: control_rel,
        paragraph: ParagraphSelector::Source,
        field: "Build-Depends".into(),
        entry: "dh-vim-addon".into(),
    }));

    // Update debian/rules: add `--with=vim_addon` to every `dh ...` recipe.
    let rules_rel = PathBuf::from("debian/rules");
    let rules_abs = base_path.join(&rules_rel);
    if rules_abs.exists() {
        let rules_content = std::fs::read_to_string(&rules_abs)?;
        let makefile = Makefile::read_relaxed(rules_content.as_bytes())
            .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;
        for rule in makefile.rules() {
            let Some(target) = rule.targets().next() else {
                continue;
            };
            for recipe_node in rule.recipe_nodes() {
                let recipe = recipe_node.text();
                let trimmed = recipe.trim();
                if !(trimmed.starts_with("dh ") || trimmed.starts_with("dh_")) {
                    continue;
                }
                let new_trimmed = dh_invoke_add_with(trimmed, "vim_addon");
                if new_trimmed == trimmed {
                    continue;
                }
                let indent: String = recipe.chars().take_while(|c| c.is_whitespace()).collect();
                let new_recipe = format!("{}{}", indent, new_trimmed);
                all_actions.push(Action::Makefile(MakefileAction::ReplaceRecipe {
                    file: rules_rel.clone(),
                    target: target.clone(),
                    recipe: recipe.to_string(),
                    new_recipe,
                }));
            }
        }
    }

    diagnostics[0].plans[0].actions = all_actions;
    Ok(diagnostics)
}

declare_fixer! {
    name: "obsolete-vim-addon-manager",
    tags: ["obsolete-vim-addon-manager"],
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
    fn test_no_control() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_vim_addon_manager() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-pkg\nMaintainer: Test User <test@example.com>\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: test-pkg\nArchitecture: all\nDepends: ${misc:Depends}, vim\nDescription: Test package\n Test description\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_removes_vim_addon_manager() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: vim-blah\nSection: editors\nPriority: optional\nMaintainer: Joe Example <joe@example.com>\nBuild-Depends: debhelper-compat (= 12)\nStandards-Version: 4.5.0\n\nPackage: vim-blah\nArchitecture: all\nDepends: ${misc:Depends}, vim, vim-addon-manager\nDescription: Blah blah\n blah\n",
        )
        .unwrap();
        let rules = debian.join("rules");
        fs::write(&rules, "#!/usr/bin/make -f\n\n%:\n\tdh $@\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Migrate from vim-addon-manager to dh-vim-addon."
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: vim-blah\nSection: editors\nPriority: optional\nMaintainer: Joe Example <joe@example.com>\nBuild-Depends: debhelper-compat (= 12), dh-vim-addon\nStandards-Version: 4.5.0\n\nPackage: vim-blah\nArchitecture: all\nDepends: ${misc:Depends}, vim\nDescription: Blah blah\n blah\n",
        );
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with=vim_addon\n",
        );
    }
}
