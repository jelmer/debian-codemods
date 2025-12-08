use crate::{declare_fixer, FixerError, FixerResult, LintianIssue, PackageType};
use debian_analyzer::control::TemplatedControlEditor;
use debversion::Version;
use makefile_lossless::Makefile;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const MINIMUM_DEBHELPER_VERSION: &str = "9.20160114";

fn check_cdbs(base_path: &Path) -> Result<bool, FixerError> {
    let rules_path = base_path.join("debian/rules");
    if !rules_path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&rules_path)?;
    Ok(content.contains("/usr/share/cdbs/"))
}

pub fn run(base_path: &Path, current_version: &Version) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");
    let rules_path = base_path.join("debian/rules");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Find -dbg packages in debian/control and check if they should be removed
    let mut editor = TemplatedControlEditor::open(&control_path)?;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut packages_to_remove = Vec::new();

    for binary in editor.binaries() {
        let paragraph = binary.as_deb822();
        if let Some(package) = paragraph.get("Package") {
            let package = package.trim();
            if package.ends_with("-dbg") {
                // Skip python*-dbg packages
                if package.starts_with("python") {
                    continue;
                }

                let line_number = paragraph.line() + 1; // Convert to 1-indexed
                let issue = LintianIssue {
                    package: Some(package.to_string()),
                    package_type: Some(PackageType::Binary),
                    tag: Some("debian-control-has-obsolete-dbg-package".to_string()),
                    info: Some(vec![format!(
                        "(in section for {}) Package [debian/control:{}]",
                        package, line_number
                    )]),
                };

                if issue.should_fix(base_path) {
                    packages_to_remove.push(package.to_string());
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
            }
        }
    }

    if packages_to_remove.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    let dbg_packages: HashSet<String> = packages_to_remove.iter().cloned().collect();

    // Remove the binaries
    for package in &packages_to_remove {
        if !editor.remove_binary(package) {
            return Err(FixerError::Other(format!(
                "Failed to remove binary: {}",
                package
            )));
        }
    }

    // Ensure minimum debhelper version
    if let Some(mut source) = editor.source() {
        let mut build_depends = source.build_depends().unwrap_or_else(|| {
            use debian_control::lossless::relations::Relations;
            Relations::new()
        });

        let minimum_version: debversion::Version = MINIMUM_DEBHELPER_VERSION.parse().unwrap();
        build_depends.ensure_minimum_version("debhelper", &minimum_version);

        source.set_build_depends(&build_depends);
    }

    // Get current version for migration
    let current_version_str = current_version.to_string();
    let migrate_version = if current_version_str.ends_with('~') {
        format!("<< {}", current_version_str)
    } else {
        format!("<< {}~", current_version_str)
    };

    // Update debian/rules
    if !rules_path.exists() {
        return Err(FixerError::Other(
            "debian/rules not found, but -dbg packages were removed".to_string(),
        ));
    }

    let content = fs::read_to_string(&rules_path)?;
    let makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    let mut dbg_migration_done = HashSet::new();
    let mut rules_uses_variables = false;

    for mut rule in makefile.rules() {
        let mut commands_to_update = Vec::new();

        for (i, recipe) in rule.recipes().enumerate() {
            let recipe_str = recipe.trim();

            if recipe_str.starts_with("dh_strip ") || recipe_str.starts_with("dh ") {
                let mut new_recipe = recipe.to_string();

                for dbg_pkg in &dbg_packages {
                    let old_arg = format!("--dbg-package={}", dbg_pkg);
                    let new_arg = format!("--dbgsym-migration='{} ({})'", dbg_pkg, migrate_version);

                    if new_recipe.contains(&old_arg) {
                        new_recipe = new_recipe.replace(&old_arg, &new_arg);
                        dbg_migration_done.insert(dbg_pkg.clone());
                    }
                }

                if new_recipe.contains('$') {
                    rules_uses_variables = true;
                }

                if new_recipe != recipe {
                    commands_to_update.push((i, new_recipe));
                }
            }
        }

        for (i, new_recipe) in commands_to_update {
            rule.replace_command(i, &new_recipe);
        }
    }

    // Check if all packages were migrated
    let difference: HashSet<_> = dbg_packages
        .symmetric_difference(&dbg_migration_done)
        .collect();

    if !difference.is_empty() {
        if check_cdbs(base_path)? {
            return Err(FixerError::Other("package uses cdbs".to_string()));
        }
        if rules_uses_variables {
            return Err(FixerError::Other(
                "rules uses variables, cannot determine how to migrate".to_string(),
            ));
        }
        return Err(FixerError::Other(format!(
            "packages missing migration: {:?}",
            difference
        )));
    }

    // Write back the modified rules file
    fs::write(&rules_path, makefile.to_string())?;

    // Commit the control file changes
    editor.commit()?;

    let package_list: Vec<_> = dbg_packages.iter().map(|s| s.as_str()).collect();
    let description = if dbg_packages.len() > 1 {
        format!(
            "Transition to automatic debug packages (from: {}).",
            package_list.join(", ")
        )
    } else {
        format!(
            "Transition to automatic debug package (from: {}).",
            package_list.join(", ")
        )
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "debian-control-has-obsolete-dbg-package",
    tags: ["debian-control-has-obsolete-dbg-package"],
    apply: |basedir, _package, version, _preferences| {
        run(basedir, version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_remove_dbg_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: mypackage
Build-Depends: debhelper (>= 9)

Package: mypackage
Architecture: any

Package: mypackage-dbg
Architecture: any
Section: debug
"#;

        let rules_content = r#"#!/usr/bin/make -f

%:
	dh $@

override_dh_strip:
	dh_strip --dbg-package=mypackage-dbg
"#;

        fs::write(debian_dir.join("control"), control_content).unwrap();
        fs::write(debian_dir.join("rules"), rules_content).unwrap();

        let version: debversion::Version = "1.0-1".parse().unwrap();
        let result = run(base_path, &version);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_control = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(!updated_control.contains("mypackage-dbg"));

        let updated_rules = fs::read_to_string(debian_dir.join("rules")).unwrap();
        assert!(updated_rules.contains("--dbgsym-migration='mypackage-dbg (<< 1.0-1~)'"));
        assert!(!updated_rules.contains("--dbg-package=mypackage-dbg"));
    }

    #[test]
    fn test_no_dbg_packages() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: mypackage
Build-Depends: debhelper (>= 9)

Package: mypackage
Architecture: any
"#;

        fs::write(debian_dir.join("control"), control_content).unwrap();

        let version: debversion::Version = "1.0-1".parse().unwrap();
        let result = run(base_path, &version);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_skip_python_dbg() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: mypackage
Build-Depends: debhelper (>= 9)

Package: mypackage
Architecture: any

Package: python3-mypackage-dbg
Architecture: any
Section: debug
"#;

        fs::write(debian_dir.join("control"), control_content).unwrap();

        let version: debversion::Version = "1.0-1".parse().unwrap();
        let result = run(base_path, &version);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
