use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use makefile_lossless::Makefile;
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let bytes = match ws.read_file(Path::new("debian/rules"))? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let makefile = Makefile::read_relaxed(bytes.as_slice())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    let mut diagnostics = Vec::new();
    for rule in makefile.rules() {
        let Some(target) = rule.targets().next() else {
            continue;
        };
        for recipe in rule.recipe_nodes() {
            let text = recipe.text();
            if text.trim() != "dh_clean -k" {
                continue;
            }
            let issue = LintianIssue::source_with_info(
                "dh-clean-k-is-deprecated",
                vec!["[debian/rules]".to_string()],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                r#"debian/rules: Use dh_prep rather than "dh_clean -k"."#,
                vec![Action::Makefile(MakefileAction::ReplaceRecipe {
                    file: rules_rel.clone(),
                    target: target.clone(),
                    recipe: text.to_string(),
                    new_recipe: "dh_prep".into(),
                })],
            ));
            break;
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "dh-clean-k-is-deprecated",
    tags: ["dh-clean-k-is-deprecated"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_replace_dh_clean_k() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\nbuild:\n\tdh_testdir\n\t$(MAKE)\n\ninstall: build\n\tdh_testdir\n\tdh_testroot\n\tdh_clean -k\n\tdh_installdirs\n\nclean:\n\tdh_clean\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            r#"debian/rules: Use dh_prep rather than "dh_clean -k"."#
        );
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\nbuild:\n\tdh_testdir\n\t$(MAKE)\n\ninstall: build\n\tdh_testdir\n\tdh_testroot\n\tdh_prep\n\tdh_installdirs\n\nclean:\n\tdh_clean\n",
        );
    }

    #[test]
    fn test_replace_indented_dh_clean_k() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(&rules, "install:\n\tdh_clean -k\n\tdh_installdirs\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "install:\n\tdh_prep\n\tdh_installdirs\n",
        );
    }

    #[test]
    fn test_no_change_when_no_dh_clean_k() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make -f\n\nbuild:\n\tdh_testdir\n\ninstall: build\n\tdh_prep\n\tdh_installdirs\n\nclean:\n\tdh_clean\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_dh_clean_k_not_standalone() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "install:\n\tdh_clean -k -a\n\tdh_installdirs\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
