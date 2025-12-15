use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_analyzer::relations::ensure_some_version;
use debian_analyzer::rules::dh_invoke_add_with;
use makefile_lossless::Makefile;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path, _preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut needs_vim_addon_sequence = false;

    // Check each binary package for vim-addon-manager dependency
    for mut binary in editor.binaries() {
        // Get Depends as a raw string
        let depends_str = binary.as_deb822().get("Depends").unwrap_or_default();

        if !depends_str.contains("vim-addon-manager") {
            continue;
        }

        let issue = LintianIssue {
            package: binary.as_deb822().get("Package").map(|s| s.to_string()),
            package_type: Some(crate::PackageType::Binary),
            tag: Some("obsolete-vim-addon-manager".to_string()),
            info: None,
        };

        if !issue.should_fix(base_path) {
            overridden_issues.push(issue);
            continue;
        }

        // Parse with substvars allowed
        let (mut new_depends, _errors) =
            debian_control::lossless::Relations::parse_relaxed(&depends_str, true);

        // Find and remove entries that match vim-addon-manager
        let entries_to_remove: Vec<usize> = new_depends
            .iter_relations_for("vim-addon-manager")
            .map(|(idx, _)| idx)
            .collect();

        for idx in entries_to_remove.into_iter().rev() {
            new_depends.remove_entry(idx);
        }

        binary.set_depends(Some(&new_depends));

        fixed_issues.push(issue);
        needs_vim_addon_sequence = true;
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Add dh-vim-addon to Build-Depends
    if let Some(mut source) = editor.source() {
        let build_depends = source.build_depends().unwrap_or_default();
        let mut new_build_depends = build_depends;
        ensure_some_version(&mut new_build_depends, "dh-vim-addon");
        source.set_build_depends(&new_build_depends);
    }

    editor.commit()?;

    // Update debian/rules to add --with=vim_addon
    if needs_vim_addon_sequence {
        let rules_path = base_path.join("debian/rules");
        if rules_path.exists() {
            let content = fs::read_to_string(&rules_path)?;
            let makefile = Makefile::read_relaxed(content.as_bytes())
                .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

            let mut rules_modified = false;
            let mut rules: Vec<_> = makefile.rules().collect();

            for rule in &mut rules {
                for (recipe_index, recipe) in rule.recipes().enumerate() {
                    let trimmed = recipe.trim();
                    if trimmed.starts_with("dh ") || trimmed.starts_with("dh_") {
                        let new_recipe = dh_invoke_add_with(trimmed, "vim_addon");
                        if new_recipe != trimmed {
                            rule.replace_command(recipe_index, &new_recipe);
                            rules_modified = true;
                        }
                    }
                }
            }

            if rules_modified {
                fs::write(&rules_path, makefile.to_string())?;
            }
        }
    }

    Ok(
        FixerResult::builder("Migrate from vim-addon-manager to dh-vim-addon.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "obsolete-vim-addon-manager",
    tags: ["obsolete-vim-addon-manager"],
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
    fn test_no_control() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_vim_addon_manager() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Maintainer: Test User <test@example.com>
Build-Depends: debhelper-compat (= 13)

Package: test-pkg
Architecture: all
Depends: ${misc:Depends}, vim
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_removes_vim_addon_manager() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: vim-blah
Section: editors
Priority: optional
Maintainer: Joe Example <joe@example.com>
Build-Depends: debhelper-compat (= 12)
Standards-Version: 4.5.0

Package: vim-blah
Architecture: all
Depends: ${misc:Depends}, vim, vim-addon-manager
Description: Blah blah
 blah
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let rules_content = r#"#!/usr/bin/make -f

%:
	dh $@
"#;
        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Migrate from vim-addon-manager to dh-vim-addon."
        );

        // Check that vim-addon-manager was removed from Depends
        let editor = TemplatedControlEditor::open(&control_path).unwrap();
        let binary = editor.binaries().next().unwrap();
        let depends_str = binary.as_deb822().get("Depends").unwrap();
        assert_eq!(depends_str, "${misc:Depends}, vim");

        // Check that dh-vim-addon was added to Build-Depends
        let source = editor.source().unwrap();
        let build_depends = source.build_depends().unwrap();
        assert_eq!(
            build_depends.to_string(),
            "debhelper-compat (= 12), dh-vim-addon"
        );

        // Check that debian/rules was updated
        let rules_content = fs::read_to_string(&rules_path).unwrap();
        assert_eq!(
            rules_content,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with=vim_addon\n"
        );
    }
}
