use crate::{declare_fixer, FixerError, FixerResult};
use makefile_lossless::Makefile;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");

    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&rules_path)?;
    let makefile: Makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    let mut made_changes = false;

    // Iterate through rules and modify them directly
    let mut rules: Vec<_> = makefile.rules().collect();
    for rule in &mut rules {
        // Check if this rule has "dh_clean -k" command
        for (recipe_index, recipe) in rule.recipes().enumerate() {
            if recipe.trim() == "dh_clean -k" {
                // Use replace_command to modify the rule in place
                if rule.replace_command(recipe_index, "dh_prep") {
                    made_changes = true;
                    break; // Only replace first occurrence per rule
                }
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    // Write back the modified makefile
    fs::write(&rules_path, makefile.to_string())?;

    Ok(
        FixerResult::builder(r#"debian/rules: Use dh_prep rather than "dh_clean -k"."#)
            .fixed_tags(vec!["dh-clean-k-is-deprecated"])
            .build(),
    )
}

declare_fixer! {
    name: "dh-clean-k-is-deprecated",
    tags: ["dh-clean-k-is-deprecated"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_replace_dh_clean_k() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = r#"#!/usr/bin/make -f

build:
	dh_testdir
	$(MAKE)

install: build
	dh_testdir
	dh_testroot
	dh_clean -k
	dh_installdirs

clean:
	dh_clean
"#;

        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        // Check that dh_clean -k was replaced with dh_prep
        let updated_content = fs::read_to_string(&rules_path).unwrap();
        assert!(!updated_content.contains("dh_clean -k"));
        assert!(updated_content.contains("dh_prep"));

        let result = result.unwrap();
        assert_eq!(
            result.description,
            r#"debian/rules: Use dh_prep rather than "dh_clean -k"."#
        );
    }

    #[test]
    fn test_replace_indented_dh_clean_k() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = "install:\n\tdh_clean -k\n\tdh_installdirs\n";
        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        // Check that dh_clean -k was replaced with dh_prep
        let updated_content = fs::read_to_string(&rules_path).unwrap();
        assert!(!updated_content.contains("dh_clean -k"));
        assert!(updated_content.contains("dh_prep"));
    }

    #[test]
    fn test_no_change_when_no_dh_clean_k() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = r#"#!/usr/bin/make -f

build:
	dh_testdir
	$(MAKE)

install: build
	dh_prep
	dh_installdirs

clean:
	dh_clean
"#;

        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_dh_clean_k_not_standalone() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = r#"install:
	dh_clean -k -a
	dh_installdirs
"#;

        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
        let temp_dir = TempDir::new().unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
