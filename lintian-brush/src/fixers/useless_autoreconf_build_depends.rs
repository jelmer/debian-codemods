use crate::diagnostic::{Action, Deb822Action, Diagnostic, MakefileAction, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_analyzer::rules::dh_invoke_drop_with;
use makefile_lossless::Makefile;
use std::path::{Path, PathBuf};

pub fn detect(
    base_path: &Path,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    // Compat 10 is required for `dh` to autoreconf by default. If the
    // target release tops out below that, we can't safely drop the addon.
    if let Some(release) = preferences.compat_release.as_ref() {
        if debian_analyzer::debhelper::maximum_debhelper_compat_version(release) < 10 {
            return Ok(Vec::new());
        }
    }

    let rules_rel = PathBuf::from("debian/rules");
    let rules_abs = base_path.join(&rules_rel);
    if !rules_abs.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&rules_abs)?;
    let makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    let mut actions: Vec<Action> = Vec::new();
    for rule in makefile.rules() {
        let Some(target) = rule.targets().next() else {
            continue;
        };
        for recipe_node in rule.recipe_nodes() {
            let recipe = recipe_node.text();
            let new_recipe = dh_invoke_drop_with(&recipe, "autoreconf");
            if new_recipe == recipe {
                continue;
            }
            actions.push(Action::Makefile(MakefileAction::ReplaceRecipe {
                file: rules_rel.clone(),
                target: target.clone(),
                recipe: recipe.to_string(),
                new_recipe,
            }));
        }
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let control_rel = PathBuf::from("debian/control");
    if base_path.join(&control_rel).exists() {
        actions.push(Action::Deb822(Deb822Action::EnsureRelation {
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            entry: "debhelper (>= 10~)".into(),
        }));
        actions.push(Action::Deb822(Deb822Action::DropRelation {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            package: "dh-autoreconf".into(),
        }));
    }

    let issue = LintianIssue::source_with_info(
        "useless-autoreconf-build-depends",
        vec!["(does not need to satisfy dh-autoreconf:any)".to_string()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Drop unnecessary dependency on dh-autoreconf.",
        actions,
    )])
}

declare_fixer! {
    name: "useless-autoreconf-build-depends",
    tags: ["useless-autoreconf-build-depends"],
    diagnose: |basedir, _package, _version, preferences| {
        detect(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_drop_autoreconf() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with=autoreconf\n",
        )
        .unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: blah\nBuild-Depends: debhelper (>= 9), dh-autoreconf\n\nPackage: blah\nDescription: blah\n blah\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n",
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\nBuild-Depends: debhelper (>= 10~)\n\nPackage: blah\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_no_autoreconf() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("rules"), "#!/usr/bin/make -f\n\n%:\n\tdh $@\n").unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper (>= 10~)\n\nPackage: blah\nDescription: blah\n blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
