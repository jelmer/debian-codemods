use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use makefile_lossless::Makefile;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");

    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Read and parse the makefile
    let content = fs::read_to_string(&rules_path)?;
    let makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    // Check if there are any $(PWD) references before making changes
    let pwd_count = content.matches("$(PWD)").count();
    if pwd_count == 0 {
        return Err(FixerError::NoChanges);
    }

    // Create issue and check if we should fix it
    let issue = LintianIssue::source_with_info(
        "debian-rules-calls-pwd",
        vec!["[debian/rules]".to_string()],
    );
    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    let mut made_changes = false;

    // Process all rules and their commands
    for mut rule in makefile.rules() {
        let mut commands_to_update = Vec::new();
        for (i, recipe) in rule.recipes().enumerate() {
            if recipe.contains("$(PWD)") {
                let new_recipe = recipe.replace("$(PWD)", "$(CURDIR)");
                commands_to_update.push((i, new_recipe));
            }
        }
        for (i, new_recipe) in commands_to_update {
            rule.replace_command(i, &new_recipe);
            made_changes = true;
        }
    }

    // For variable definitions, we need to do a text replacement in the entire file
    // since makefile-lossless doesn't provide a direct way to modify variable values
    let mut result_content = makefile.to_string();

    // Check if there are any $(PWD) in variable definitions that weren't in rules
    if result_content.contains("$(PWD)") {
        result_content = result_content.replace("$(PWD)", "$(CURDIR)");
        made_changes = true;
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    // Write the updated content back
    fs::write(&rules_path, result_content)?;

    Ok(
        FixerResult::builder("debian/rules: Avoid using $(PWD) variable.")
            .fixed_issues(vec![issue])
            .build(),
    )
}

declare_fixer! {
    name: "debian-rules-should-not-use-pwd",
    tags: ["debian-rules-calls-pwd"],
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
    fn test_replace_pwd_with_curdir() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(
            &rules_path,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_auto_install:\n\tdh_auto_install --destdir=$(PWD)/debian/tmp\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "debian/rules: Avoid using $(PWD) variable."
        );

        let content = fs::read_to_string(&rules_path).unwrap();
        assert!(content.contains("$(CURDIR)/debian/tmp"));
        assert!(!content.contains("$(PWD)"));
    }

    #[test]
    fn test_multiple_pwd_occurrences() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(
            &rules_path,
            "#!/usr/bin/make -f\n\nFOO=$(PWD)/foo\nBAR=$(PWD)/bar\n\n%:\n\tdh $@\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "debian/rules: Avoid using $(PWD) variable."
        );

        let content = fs::read_to_string(&rules_path).unwrap();
        assert!(content.contains("FOO=$(CURDIR)/foo"));
        assert!(content.contains("BAR=$(CURDIR)/bar"));
        assert!(!content.contains("$(PWD)"));
    }

    #[test]
    fn test_no_pwd_in_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(
            &rules_path,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\noverride_dh_auto_install:\n\tdh_auto_install --destdir=$(CURDIR)/debian/tmp\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_pwd_in_command() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(
            &rules_path,
            "#!/usr/bin/make -f\n\ntest:\n\techo $(PWD)\n\tcp $(PWD)/file dest/\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "debian/rules: Avoid using $(PWD) variable."
        );

        let content = fs::read_to_string(&rules_path).unwrap();
        assert!(content.contains("echo $(CURDIR)"));
        assert!(content.contains("cp $(CURDIR)/file"));
        assert!(!content.contains("$(PWD)"));
    }

    #[test]
    fn test_pwd_in_variable_and_command() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(
            &rules_path,
            "#!/usr/bin/make -f\n\nBUILDDIR=$(PWD)/build\n\noverride_dh_auto_configure:\n\tdh_auto_configure -- --prefix=$(PWD)/debian/tmp\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "debian/rules: Avoid using $(PWD) variable."
        );

        let content = fs::read_to_string(&rules_path).unwrap();
        assert!(content.contains("BUILDDIR=$(CURDIR)/build"));
        assert!(content.contains("--prefix=$(CURDIR)/debian/tmp"));
        assert!(!content.contains("$(PWD)"));
    }
}
