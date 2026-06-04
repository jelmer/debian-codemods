use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use makefile_lossless::{Makefile, Parse};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn get_archs(ws: &dyn Workspace) -> Result<HashSet<String>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(HashSet::new()),
        Err(e) => return Err(e.into()),
    };
    Ok(control
        .binaries()
        .filter_map(|b| b.architecture().map(|a| a.to_string()))
        .collect())
}

fn check_cdbs_ws(ws: &dyn Workspace) -> Result<bool, FixerError> {
    let Some(content) = ws.read_file(Path::new("debian/rules"))? else {
        return Ok(false);
    };
    Ok(content
        .windows(b"/usr/share/cdbs/".len())
        .any(|w| w == b"/usr/share/cdbs/"))
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let rules_bytes = match ws.read_file(Path::new("debian/rules"))? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = std::str::from_utf8(&rules_bytes) else {
        return Ok(Vec::new());
    };
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
    if check_cdbs_ws(ws)? || makefile.includes().count() > 0 {
        return Ok(Vec::new());
    }

    let archs = get_archs(ws)?;
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
                Visibility::Warning,
                vec!["build-indep [debian/rules]".to_string()],
            ),
            "debian/rules is missing recommended target build-indep.",
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
                Visibility::Warning,
                vec!["build-arch [debian/rules]".to_string()],
            ),
            "debian/rules is missing recommended target build-arch.",
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
        for plan in &mut d.plans {
            plan.label = summary.clone();
        }
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "debian-rules-missing-recommended-target",
    tags: ["debian-rules-missing-recommended-target"],
    triggers: [
        debian_workspace::Trigger::File("debian/rules"),
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Architecture",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
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
    fn test_parses_define_endef_block() {
        // A `define`/`endef` block used to trip up the makefile parser, which
        // made the fixer skip with a spurious "parse errors" warning.
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\ndefine COMMON_CONFIGURE_ARGS\n\t--prefix=/usr\n\t--disable-static\nendef\n\nbuild: blah\n\t$(MAKE) install\n",
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
            "#!/usr/bin/make -f\n\ndefine COMMON_CONFIGURE_ARGS\n\t--prefix=/usr\n\t--disable-static\nendef\n\nbuild: blah\n\t$(MAKE) install\n\nbuild-indep:\n\nbuild-arch: build\n",
        );
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
