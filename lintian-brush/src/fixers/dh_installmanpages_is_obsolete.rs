use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

const DESCRIPTION: &str = r#"debian/rules calls deprecated "dh_installmanpages"."#;
const LABEL: &str = "debian/rules: Use dh_installman rather than dh_installmanpages.";

/// Whether the package carries a `dh_installman` man-page list.
///
/// `dh_installmanpages` auto-discovers man pages in the build tree;
/// `dh_installman` instead installs the man pages named in
/// `debian/manpages` / `debian/<package>.manpages` (or on its command
/// line). Renaming the call only preserves behaviour when such a list
/// exists — otherwise the migrated `dh_installman` would have nothing to
/// install. The presence of a `.manpages` file is the clearest signal
/// that the maintainer has set `dh_installman` up.
fn has_manpages_list(ws: &dyn Workspace) -> bool {
    match ws.list_dir(Path::new("debian")) {
        Ok(Some(entries)) => entries
            .iter()
            .any(|e| e == "manpages" || e.ends_with(".manpages")),
        // Missing directory, or a host that can't enumerate it: stay on
        // the safe side and treat the rename as unsafe.
        _ => false,
    }
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    // Collect every bare `dh_installmanpages` recipe. Only the bare
    // command is a candidate: dh_installmanpages takes its file
    // arguments as man pages to *exclude*, whereas dh_installman takes
    // them as man pages to *install*, so a call carrying arguments
    // cannot be migrated by a plain rename.
    let mut occurrences: Vec<(String, String)> = Vec::new();
    for rule in makefile.rules() {
        let Some(target) = rule.targets().next() else {
            continue;
        };
        for recipe in rule.recipe_nodes() {
            let text = recipe.text();
            if text.trim() == "dh_installmanpages" {
                occurrences.push((target.clone(), text.to_string()));
            }
        }
    }
    if occurrences.is_empty() {
        return Ok(Vec::new());
    }

    // The diagnostic is always reported, but a fix plan is only attached
    // when renaming the call is behaviour-preserving. Without a man-page
    // list, a plain rename would drop the package's man pages, so the
    // diagnostic is emitted plan-less: lintian-brush flags the obsolete
    // call but leaves the rules file untouched.
    let safe_to_rename = has_manpages_list(ws);

    Ok(occurrences
        .into_iter()
        .map(|(target, recipe)| {
            let issue = LintianIssue::source_with_info(
                "dh_installmanpages-is-obsolete",
                Visibility::Warning,
                vec!["[debian/rules]".to_string()],
            );
            if safe_to_rename {
                Diagnostic::with_actions(
                    issue,
                    DESCRIPTION,
                    LABEL,
                    vec![Action::Makefile(MakefileAction::ReplaceRecipe {
                        file: rules_rel.clone(),
                        target,
                        recipe,
                        new_recipe: "dh_installman".into(),
                    })],
                )
            } else {
                Diagnostic::with_plans(issue, DESCRIPTION, Vec::new())
            }
        })
        .collect())
}

declare_detector! {
    name: "dh-installmanpages-is-obsolete",
    tags: ["dh_installmanpages-is-obsolete"],
    triggers: [debian_workspace::Trigger::File("debian/rules")],
    cost: crate::detector::DetectorCost::Filesystem,
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn workspace(base: &Path) -> debian_workspace::fs_workspace::FsWorkspace {
        let version: Version = "1.0".parse().unwrap();
        debian_workspace::fs_workspace::FsWorkspace::new(base, Some("test".into()), Some(version))
    }

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        DetectorImpl.apply(&workspace(base), &FixerPreferences::default())
    }

    const RULES_WITH_DH_INSTALLMANPAGES: &str = "#!/usr/bin/make -f\n\nbuild:\n\tdh_testdir\n\t$(MAKE)\n\ninstall: build\n\tdh_testdir\n\tdh_installmanpages\n\tdh_installdirs\n\nclean:\n\tdh_clean\n";

    #[test]
    fn test_replace_dh_installmanpages_with_manpages_list() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("rules"), RULES_WITH_DH_INSTALLMANPAGES).unwrap();
        // A man-page list is present, so the rename is behaviour-preserving.
        fs::write(debian.join("manpages"), "foo.1\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, LABEL);
        assert_eq!(
            fs::read_to_string(debian.join("rules")).unwrap(),
            "#!/usr/bin/make -f\n\nbuild:\n\tdh_testdir\n\t$(MAKE)\n\ninstall: build\n\tdh_testdir\n\tdh_installman\n\tdh_installdirs\n\nclean:\n\tdh_clean\n",
        );
    }

    #[test]
    fn test_replace_with_package_specific_manpages_list() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("rules"), RULES_WITH_DH_INSTALLMANPAGES).unwrap();
        fs::write(debian.join("mypkg.manpages"), "foo.1\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(fs::read_to_string(debian.join("rules"))
            .unwrap()
            .contains("\tdh_installman\n"));
    }

    #[test]
    fn test_diagnostic_without_plan_when_no_manpages_list() {
        // dh_installmanpages is present but no debian/*.manpages list
        // exists: the issue is still reported, but as a plan-less
        // diagnostic — no fix is attached.
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("rules"), RULES_WITH_DH_INSTALLMANPAGES).unwrap();

        let diagnostics = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].plans.is_empty(),
            "expected a plan-less diagnostic when the rename is unsafe"
        );
        assert_eq!(
            diagnostics[0].issue.as_ref().and_then(|i| i.tag.as_deref()),
            Some("dh_installmanpages-is-obsolete")
        );
    }

    #[test]
    fn test_no_fix_applied_without_manpages_list() {
        // The plan-less diagnostic leaves the rules file untouched when
        // run through the batch applier.
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("rules"), RULES_WITH_DH_INSTALLMANPAGES).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(debian.join("rules")).unwrap(),
            RULES_WITH_DH_INSTALLMANPAGES
        );
    }

    #[test]
    fn test_no_change_when_no_dh_installmanpages() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make -f\n\ninstall: build\n\tdh_installman\n\tdh_installdirs\n",
        )
        .unwrap();
        fs::write(debian.join("manpages"), "foo.1\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_dh_installmanpages_has_arguments() {
        // Arguments to dh_installmanpages are man pages to exclude; the
        // call cannot be migrated by a plain rename, so it is left alone.
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "install:\n\tdh_installmanpages foo.1 bar.8\n\tdh_installdirs\n",
        )
        .unwrap();
        fs::write(debian.join("manpages"), "foo.1\n").unwrap();
        let diagnostics = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_no_rules_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
