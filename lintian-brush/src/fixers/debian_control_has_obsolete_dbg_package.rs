use crate::diagnostic::{Action, Deb822Action, Diagnostic, MakefileAction, ParagraphSelector};
use crate::{FixerError, LintianIssue, PackageType, Version};
use debian_control::lossless::Control;
use makefile_lossless::Makefile;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const MINIMUM_DEBHELPER_VERSION: &str = "9.20160114";

fn check_cdbs(base_path: &Path) -> bool {
    let rules_path = base_path.join("debian/rules");
    if !rules_path.exists() {
        return false;
    }
    std::fs::read_to_string(&rules_path)
        .map(|c| c.contains("/usr/share/cdbs/"))
        .unwrap_or(false)
}

pub fn detect(base_path: &Path, current_version: &Version) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_abs = base_path.join(&control_rel);
    if !control_abs.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&control_abs)?;
    let Ok(control) = Control::from_str(&content) else {
        return Ok(Vec::new());
    };

    let mut dbg_packages: Vec<String> = Vec::new();
    let mut issues: Vec<LintianIssue> = Vec::new();
    for binary in control.binaries() {
        let Some(name) = binary.name() else {
            continue;
        };
        if !name.ends_with("-dbg") {
            continue;
        }
        if name.starts_with("python") {
            continue;
        }
        let line_number = binary.as_deb822().line() + 1;
        issues.push(LintianIssue {
            package: Some(name.clone()),
            package_type: Some(PackageType::Binary),
            tag: Some("debian-control-has-obsolete-dbg-package".to_string()),
            info: Some(format!(
                "(in section for {}) Package [debian/control:{}]",
                name, line_number
            )),
        });
        dbg_packages.push(name);
    }

    if dbg_packages.is_empty() {
        return Ok(Vec::new());
    }

    let rules_rel = PathBuf::from("debian/rules");
    let rules_abs = base_path.join(&rules_rel);
    if !rules_abs.exists() {
        return Ok(Vec::new());
    }

    let current_version_str = current_version.to_string();
    let migrate_version = if current_version_str.ends_with('~') {
        format!("<< {}", current_version_str)
    } else {
        format!("<< {}~", current_version_str)
    };

    let rules_content = std::fs::read_to_string(&rules_abs)?;
    let makefile = Makefile::read_relaxed(rules_content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    let mut recipe_actions: Vec<Action> = Vec::new();
    let mut migrated: HashSet<String> = HashSet::new();
    let mut rules_uses_variables = false;
    for rule in makefile.rules() {
        let Some(target) = rule.targets().next() else {
            continue;
        };
        for recipe_node in rule.recipe_nodes() {
            let recipe = recipe_node.text();
            let trimmed = recipe.trim();
            if !trimmed.starts_with("dh_strip ") && !trimmed.starts_with("dh ") {
                continue;
            }
            let mut new_recipe = recipe.to_string();
            for dbg in &dbg_packages {
                let old = format!("--dbg-package={}", dbg);
                let new = format!("--dbgsym-migration='{} ({})'", dbg, migrate_version);
                if new_recipe.contains(&old) {
                    new_recipe = new_recipe.replace(&old, &new);
                    migrated.insert(dbg.clone());
                }
            }
            if new_recipe.contains('$') {
                rules_uses_variables = true;
            }
            if new_recipe == recipe {
                continue;
            }
            recipe_actions.push(Action::Makefile(MakefileAction::ReplaceRecipe {
                file: rules_rel.clone(),
                target: target.clone(),
                recipe: recipe.to_string(),
                new_recipe,
            }));
        }
    }

    let needed: HashSet<String> = dbg_packages.iter().cloned().collect();
    if needed != migrated {
        if check_cdbs(base_path) {
            return Ok(Vec::new()); // CDBS uses different mechanisms
        }
        if rules_uses_variables {
            return Ok(Vec::new()); // Can't safely transform variable-based invocations
        }
        return Ok(Vec::new()); // Some packages couldn't be migrated
    }

    let mut all_actions: Vec<Action> = Vec::new();
    all_actions.extend(recipe_actions);
    for pkg in &dbg_packages {
        all_actions.push(Action::Deb822(Deb822Action::RemoveParagraph {
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Binary {
                package: pkg.clone(),
            },
        }));
    }
    all_actions.push(Action::Deb822(Deb822Action::EnsureRelation {
        file: control_rel,
        paragraph: ParagraphSelector::Source,
        field: "Build-Depends".into(),
        entry: format!("debhelper (>= {})", MINIMUM_DEBHELPER_VERSION),
    }));

    let summary = if dbg_packages.len() > 1 {
        format!(
            "Transition to automatic debug packages (from: {}).",
            dbg_packages.join(", ")
        )
    } else {
        format!(
            "Transition to automatic debug package (from: {}).",
            dbg_packages.join(", ")
        )
    };

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for (i, issue) in issues.into_iter().enumerate() {
        let plan_actions = if i == 0 {
            all_actions.clone()
        } else {
            Vec::new()
        };
        diagnostics.push(Diagnostic::with_actions(
            issue,
            summary.clone(),
            plan_actions,
        ));
    }
    Ok(diagnostics)
}

declare_fixer! {
    name: "debian-control-has-obsolete-dbg-package",
    tags: ["debian-control-has-obsolete-dbg-package"],
    diagnose: |basedir, _package, version, _preferences| {
        detect(basedir, version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::FixerPreferences;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path, version: &str) -> Result<crate::FixerResult, FixerError> {
        let v: Version = version.parse().unwrap();
        FixerImpl.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_dbg_package() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: mypackage\nBuild-Depends: debhelper (>= 9)\n\nPackage: mypackage\nArchitecture: any\nDescription: test\n test\n\nPackage: mypackage-dbg\nArchitecture: any\nSection: debug\nDescription: dbg\n test\n",
        )
        .unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_strip:\n\tdh_strip --dbg-package=mypackage-dbg\n",
        )
        .unwrap();

        run_apply(tmp.path(), "1.0-1").unwrap();
        let updated_control = fs::read_to_string(&control).unwrap();
        assert!(!updated_control.contains("mypackage-dbg"));
        assert!(updated_control.contains("debhelper (>= 9.20160114)"));

        let updated_rules = fs::read_to_string(&rules).unwrap();
        assert!(updated_rules.contains("--dbgsym-migration='mypackage-dbg (<< 1.0-1~)'"));
        assert!(!updated_rules.contains("--dbg-package=mypackage-dbg"));
    }

    #[test]
    fn test_no_dbg_packages() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: mypackage\nBuild-Depends: debhelper (>= 9)\n\nPackage: mypackage\nArchitecture: any\nDescription: test\n test\n",
        )
        .unwrap();
        assert!(matches!(
            run_apply(tmp.path(), "1.0-1"),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_skip_python_dbg() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: mypackage\nBuild-Depends: debhelper (>= 9)\n\nPackage: mypackage\nArchitecture: any\nDescription: test\n test\n\nPackage: python3-mypackage-dbg\nArchitecture: any\nSection: debug\nDescription: pkg\n test\n",
        )
        .unwrap();
        assert!(matches!(
            run_apply(tmp.path(), "1.0-1"),
            Err(FixerError::NoChanges)
        ));
    }
}
