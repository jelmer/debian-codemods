use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const CLEAN_TARGETS: &[&str] = &[
    "clean",
    "distclean",
    "realclean",
    "maintainer-clean",
    "mostlyclean",
];

fn is_simple_dir(dir: &str) -> bool {
    !dir.is_empty()
        && dir
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./$(){}".contains(c))
}

/// Rewrite a recipe like `-$(MAKE) [-C dir] distclean` so that it only
/// skips the clean when the Makefile is missing, rather than ignoring all
/// errors.
///
/// Returns `None` for anything other than that exact shape, since other
/// options or shell constructs may change which Makefile is used.
fn guard_clean_recipe(text: &str) -> Option<String> {
    let rest = text.strip_prefix('-')?;
    // lintian requires the make command to follow the "-" directly.
    if rest.starts_with(char::is_whitespace) {
        return None;
    }
    let mut tokens = rest.split_whitespace();
    let make = tokens.next()?;
    if !matches!(make, "$(MAKE)" | "${MAKE}" | "make") {
        return None;
    }
    let mut dir = None;
    let mut has_target = false;
    while let Some(token) = tokens.next() {
        if token == "-C" && dir.is_none() && !has_target {
            let d = tokens.next()?;
            if !is_simple_dir(d) {
                return None;
            }
            dir = Some(d);
        } else if CLEAN_TARGETS.contains(&token) {
            has_target = true;
        } else {
            return None;
        }
    }
    if !has_target {
        return None;
    }
    let makefile = match dir {
        Some(d) => format!("{}/Makefile", d.trim_end_matches('/')),
        None => "Makefile".to_string(),
    };
    Some(format!("[ ! -f {} ] || {}", makefile, rest))
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

    let mut diagnostics = Vec::new();
    for rule in makefile.rules() {
        let Some(target) = rule.targets().next() else {
            continue;
        };
        for recipe in rule.recipe_nodes() {
            if !recipe.is_ignore_errors() {
                continue;
            }
            let text = recipe.text();
            let Some(new_recipe) = guard_clean_recipe(&text) else {
                continue;
            };
            let issue = LintianIssue::source_with_info(
                "debian-rules-ignores-make-clean-error",
                Visibility::Warning,
                vec![format!("[debian/rules:{}]", recipe.line() + 1)],
            );
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    "debian/rules ignores all errors from make clean.",
                    "debian/rules: Only skip make clean when the Makefile is missing, rather than ignoring all errors.",
                    vec![Action::Makefile(MakefileAction::ReplaceRecipe {
                        file: rules_rel.clone(),
                        target: target.clone(),
                        recipe: text,
                        new_recipe,
                    })],
                )
                // The clean target may genuinely fail for other reasons,
                // which would now break the build.
                .with_certainty(Certainty::Likely),
            );
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-rules-ignores-make-clean-error",
    tags: ["debian-rules-ignores-make-clean-error"],
    triggers: [debian_workspace::Trigger::File("debian/rules")],
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

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    #[test]
    fn test_guard_clean_recipe() {
        assert_eq!(
            guard_clean_recipe("-$(MAKE) distclean"),
            Some("[ ! -f Makefile ] || $(MAKE) distclean".to_string())
        );
        assert_eq!(
            guard_clean_recipe("-${MAKE} clean"),
            Some("[ ! -f Makefile ] || ${MAKE} clean".to_string())
        );
        assert_eq!(
            guard_clean_recipe("-make  clean distclean"),
            Some("[ ! -f Makefile ] || make  clean distclean".to_string())
        );
        assert_eq!(
            guard_clean_recipe("-$(MAKE) -C build/ distclean"),
            Some("[ ! -f build/Makefile ] || $(MAKE) -C build/ distclean".to_string())
        );
        assert_eq!(
            guard_clean_recipe("-$(MAKE) -C $(CURDIR)/src clean"),
            Some("[ ! -f $(CURDIR)/src/Makefile ] || $(MAKE) -C $(CURDIR)/src clean".to_string())
        );
    }

    #[test]
    fn test_guard_clean_recipe_backs_off() {
        assert_eq!(guard_clean_recipe("$(MAKE) distclean"), None);
        assert_eq!(guard_clean_recipe("$(MAKE) -i distclean"), None);
        assert_eq!(guard_clean_recipe("@-$(MAKE) distclean"), None);
        assert_eq!(guard_clean_recipe("- $(MAKE) distclean"), None);
        assert_eq!(guard_clean_recipe("-$(MAKE)"), None);
        assert_eq!(guard_clean_recipe("-$(MAKE) install"), None);
        assert_eq!(guard_clean_recipe("-$(MAKE) -f Makefile.foo clean"), None);
        assert_eq!(guard_clean_recipe("-$(MAKE) -Cbuild clean"), None);
        assert_eq!(guard_clean_recipe("-$(MAKE) -C build"), None);
        assert_eq!(guard_clean_recipe("-$(MAKE) -C 'a b' clean"), None);
        assert_eq!(guard_clean_recipe("-$(MAKE) clean -C build"), None);
        assert_eq!(guard_clean_recipe("-$(MAKE) clean || true"), None);
        assert_eq!(guard_clean_recipe("-$(MAKE) clean # comment"), None);
        assert_eq!(guard_clean_recipe("-$(MAKE) clean \\\n\t\tFOO=1"), None);
        assert_eq!(guard_clean_recipe("-$(MAKE) V=1 clean"), None);
        assert_eq!(guard_clean_recipe("-cmake clean"), None);
    }

    #[test]
    fn test_fix() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\nbuild:\n\t$(MAKE)\n\nclean:\n\tdh_testdir\n\t-$(MAKE) distclean\n\tdh_clean\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "debian/rules: Only skip make clean when the Makefile is missing, rather than ignoring all errors."
        );
        assert_eq!(result.certainty, Some(Certainty::Likely));
        assert_eq!(
            result
                .fixed_lintian_issues
                .iter()
                .map(|i| i.info.clone())
                .collect::<Vec<_>>(),
            vec![Some("[debian/rules:8]".to_string())]
        );
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\nbuild:\n\t$(MAKE)\n\nclean:\n\tdh_testdir\n\t[ ! -f Makefile ] || $(MAKE) distclean\n\tdh_clean\n",
        );
    }

    #[test]
    fn test_no_change_with_unsupported_form() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make -f\n\nclean:\n\t-$(MAKE) -f Makefile.custom clean\n\t$(MAKE) -i distclean\n\tdh_clean\n",
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
