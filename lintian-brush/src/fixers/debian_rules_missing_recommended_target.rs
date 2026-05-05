use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::{FixerError, LintianIssue};
use debian_analyzer::rules::check_cdbs;
use debian_control::Control;
use makefile_lossless::{Makefile, Parse};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;

fn get_archs(base_path: &Path) -> Result<HashSet<String>, FixerError> {
    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Ok(HashSet::new());
    }
    let content = std::fs::read_to_string(&control_path)?;
    let control = Control::from_str(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse control file: {}", e)))?;
    Ok(control
        .binaries()
        .filter_map(|b| b.architecture().map(|a| a.to_string()))
        .collect())
}

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let rules_abs = base_path.join(&rules_rel);
    if !rules_abs.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&rules_abs)?;
    let parsed = Parse::<Makefile>::parse_makefile(&content);
    if !parsed.ok() {
        tracing::warn!(
            "debian/rules has parse errors, skipping: {}",
            parsed
                .errors()
                .iter()
                .map(|e| format!("line {}: {}", e.line, e.message))
                .collect::<Vec<_>>()
                .join(", ")
        );
        return Ok(Vec::new());
    }
    let makefile = parsed.tree();

    let has_build_arch = makefile.find_rule_by_target_pattern("build-arch").is_some();
    let has_build_indep = makefile
        .find_rule_by_target_pattern("build-indep")
        .is_some();
    if has_build_arch && has_build_indep {
        return Ok(Vec::new());
    }

    // Includes (especially CDBS) can supply these targets out-of-band; we
    // can't see through them.
    if check_cdbs(&rules_abs) || makefile.includes().count() > 0 {
        return Ok(Vec::new());
    }

    let archs = get_archs(base_path)?;
    let phony_present = makefile.find_rule_by_target(".PHONY").is_some();

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut added: Vec<&str> = Vec::new();

    if !has_build_indep {
        let mut prereqs = Vec::new();
        if archs.contains("all") {
            prereqs.push("build".to_string());
        }
        let mut actions: Vec<Action> = vec![Action::Makefile(MakefileAction::AddRule {
            file: rules_rel.clone(),
            target: "build-indep".into(),
            prerequisites: prereqs,
        })];
        if phony_present {
            actions.push(Action::Makefile(MakefileAction::AddPhonyTarget {
                file: rules_rel.clone(),
                target: "build-indep".into(),
            }));
        }
        diagnostics.push(Diagnostic::with_actions(
            LintianIssue::source_with_info(
                "debian-rules-missing-recommended-target",
                vec!["build-indep [debian/rules]".to_string()],
            ),
            String::new(),
            actions,
        ));
        added.push("build-indep");
    }

    if !has_build_arch {
        let mut prereqs = Vec::new();
        if archs.iter().any(|a| a != "all") {
            prereqs.push("build".to_string());
        }
        let mut actions: Vec<Action> = vec![Action::Makefile(MakefileAction::AddRule {
            file: rules_rel.clone(),
            target: "build-arch".into(),
            prerequisites: prereqs,
        })];
        if phony_present {
            actions.push(Action::Makefile(MakefileAction::AddPhonyTarget {
                file: rules_rel.clone(),
                target: "build-arch".into(),
            }));
        }
        diagnostics.push(Diagnostic::with_actions(
            LintianIssue::source_with_info(
                "debian-rules-missing-recommended-target",
                vec!["build-arch [debian/rules]".to_string()],
            ),
            String::new(),
            actions,
        ));
        added.push("build-arch");
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    let summary = if added.len() == 1 {
        format!("Add missing debian/rules target {}.", added[0])
    } else {
        format!("Add missing debian/rules targets {}.", added.join(", "))
    };
    for d in &mut diagnostics {
        d.message = summary.clone();
    }
    Ok(diagnostics)
}

declare_fixer! {
    name: "debian-rules-missing-recommended-target",
    tags: ["debian-rules-missing-recommended-target"],
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
    fn test_adds_missing_targets() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\nbuild: blah\n\t$(MAKE) install\n\nclean:\n\tdh_prep -k\n",
        )
        .unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Add missing debian/rules targets build-indep, build-arch."
        );
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\nbuild: blah\n\t$(MAKE) install\n\nclean:\n\tdh_prep -k\n\nbuild-indep:\n\nbuild-arch: build\n",
        );
    }

    #[test]
    fn test_no_change_with_wildcard_rule() {
        // `%:` matches anything, including build-arch/build-indep, so the
        // missing-target check is satisfied and we shouldn't add new rules.
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("rules"), "#!/usr/bin/make -f\n\n%:\n\tdh $@\n").unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_targets_present() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\nbuild-arch:\n\nbuild-indep:\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
