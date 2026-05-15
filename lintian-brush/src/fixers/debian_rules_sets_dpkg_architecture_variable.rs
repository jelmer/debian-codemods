use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

const ARCHITECTURE_MK_PATH: &str = "/usr/share/dpkg/architecture.mk";

lazy_static! {
    static ref DPKG_ARCH_VARIABLES: HashSet<String> = {
        let mut vars = HashSet::new();
        for machine in &["BUILD", "HOST", "TARGET"] {
            for var in &[
                "ARCH",
                "ARCH_ABI",
                "ARCH_LIBC",
                "ARCH_OS",
                "ARCH_CPU",
                "ARCH_BITS",
                "ARCH_ENDIAN",
                "GNU_CPU",
                "GNU_SYSTEM",
                "GNU_TYPE",
                "MULTIARCH",
            ] {
                vars.insert(format!("DEB_{}_{}", machine, var));
            }
        }
        vars
    };
    static ref DPKG_ARCH_CALL_REGEX: Regex =
        Regex::new(r"^\$\(shell\s+dpkg-architecture\s+-q([A-Z_]+)\)$").unwrap();
}

fn is_standard_dpkg_arch_call(name: &str, value: &str) -> bool {
    DPKG_ARCH_CALL_REGEX
        .captures(value.trim())
        .map(|c| &c[1] == name)
        .unwrap_or(false)
}

pub fn detect(
    ws: &dyn Workspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let opinionated = preferences.opinionated.unwrap_or(false);

    let rules_rel = PathBuf::from("debian/rules");
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let already_included = makefile.included_files().any(|f| f == ARCHITECTURE_MK_PATH);

    let mut issues: Vec<LintianIssue> = Vec::new();
    let mut actions: Vec<Action> = Vec::new();
    let mut to_remove: Vec<String> = Vec::new();
    // First DEB_*_* variable that uses a non-standard call. In
    // opinionated mode we anchor the include directive in front of it
    // so the include lands near the section it logically supports.
    let mut first_different_var: Option<String> = None;

    for var_def in makefile.variable_definitions() {
        let Some(name) = var_def.name() else {
            continue;
        };
        if !DPKG_ARCH_VARIABLES.contains(&name) {
            continue;
        }
        let Some(value) = var_def.raw_value() else {
            continue;
        };
        if !is_standard_dpkg_arch_call(&name, &value) {
            if opinionated && first_different_var.is_none() {
                first_different_var = Some(name);
            }
            continue;
        }
        let assignment_op = var_def.assignment_operator();
        let is_hard = assignment_op.as_deref() != Some("?=");
        let line_num = var_def.line() + 1;
        let issue = LintianIssue::source_with_info(
            "debian-rules-sets-dpkg-architecture-variable",
            Visibility::Warning,
            vec![format!("{} [debian/rules:{}]", name, line_num)],
        );

        if opinionated {
            if is_hard {
                issues.push(issue);
            }
            to_remove.push(name);
        } else if is_hard {
            issues.push(issue);
            if already_included {
                to_remove.push(name);
            } else {
                actions.push(Action::Makefile(MakefileAction::SetVariableOperator {
                    file: rules_rel.clone(),
                    name,
                    operator: "?=".into(),
                }));
            }
        }
    }

    // In opinionated mode without an existing include, place
    // `include /usr/share/dpkg/architecture.mk`:
    //  - just before the first non-standard DEB_*_* variable, if any
    //    (so the include sits next to the code that depends on it), or
    //  - in place of the first variable being removed otherwise.
    let needs_include = opinionated && !to_remove.is_empty() && !already_included;
    if needs_include {
        if let Some(anchor) = first_different_var.as_ref() {
            actions.push(Action::Makefile(
                MakefileAction::InsertIncludeBeforeVariable {
                    file: rules_rel.clone(),
                    path: ARCHITECTURE_MK_PATH.into(),
                    before_variable: anchor.clone(),
                },
            ));
            for name in &to_remove {
                actions.push(Action::Makefile(MakefileAction::RemoveVariable {
                    file: rules_rel.clone(),
                    name: name.clone(),
                }));
            }
        } else {
            let (first, rest) = to_remove.split_first().unwrap();
            actions.push(Action::Makefile(
                MakefileAction::ReplaceVariableWithInclude {
                    file: rules_rel.clone(),
                    name: first.clone(),
                    path: ARCHITECTURE_MK_PATH.into(),
                },
            ));
            for name in rest {
                actions.push(Action::Makefile(MakefileAction::RemoveVariable {
                    file: rules_rel.clone(),
                    name: name.clone(),
                }));
            }
        }
    } else {
        for name in to_remove.iter() {
            actions.push(Action::Makefile(MakefileAction::RemoveVariable {
                file: rules_rel.clone(),
                name: name.clone(),
            }));
        }
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let label = if opinionated {
        "Rely on pre-initialized dpkg-architecture variables."
    } else if !to_remove.is_empty() && already_included {
        "Rely on existing architecture.mk include."
    } else {
        "Use ?= for assignments to architecture variables."
    };
    let description = "Use ?= for assignments to architecture variables.";

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    if issues.is_empty() {
        diagnostics.push(Diagnostic::untagged(description, label, actions));
    } else {
        for (i, issue) in issues.into_iter().enumerate() {
            let plan_actions = if i == 0 {
                std::mem::take(&mut actions)
            } else {
                Vec::new()
            };
            diagnostics.push(Diagnostic::with_actions(
                issue,
                description,
                label,
                plan_actions,
            ));
        }
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "debian-rules-sets-dpkg-architecture-variable",
    tags: ["debian-rules-sets-dpkg-architecture-variable"],
    triggers: [debian_workspace::Trigger::File("debian/rules")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path, opinionated: bool) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let prefs = FixerPreferences {
            opinionated: Some(opinionated),
            ..Default::default()
        };
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(v.clone()),
            );
            adapter.apply(&ws, &prefs)
        }
    }

    #[test]
    fn test_is_standard_dpkg_arch_call() {
        assert!(is_standard_dpkg_arch_call(
            "DEB_HOST_ARCH",
            "$(shell dpkg-architecture -qDEB_HOST_ARCH)"
        ));
        assert!(!is_standard_dpkg_arch_call(
            "DEB_HOST_ARCH",
            "$(shell dpkg-architecture -qDEB_BUILD_ARCH)"
        ));
        assert!(!is_standard_dpkg_arch_call("DEB_HOST_ARCH", "foo"));
    }

    #[test]
    fn test_dpkg_arch_variables() {
        assert!(DPKG_ARCH_VARIABLES.contains("DEB_HOST_ARCH"));
        assert!(DPKG_ARCH_VARIABLES.contains("DEB_BUILD_GNU_TYPE"));
        assert!(DPKG_ARCH_VARIABLES.contains("DEB_TARGET_MULTIARCH"));
        assert!(!DPKG_ARCH_VARIABLES.contains("DEB_HOST_FOO"));
    }

    #[test]
    fn test_non_opinionated_hard_assignment() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#! /usr/bin/make -f\n\nDEB_HOST_ARCH := $(shell dpkg-architecture -qDEB_HOST_ARCH)\n\n%:\n\tdh $@\n",
        )
        .unwrap();

        let result = run_apply(tmp.path(), false).unwrap();
        assert_eq!(
            result.description,
            "Use ?= for assignments to architecture variables."
        );
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#! /usr/bin/make -f\n\nDEB_HOST_ARCH ?= $(shell dpkg-architecture -qDEB_HOST_ARCH)\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_opinionated_removes_line() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#! /usr/bin/make -f\n\nDEB_HOST_ARCH := $(shell dpkg-architecture -qDEB_HOST_ARCH)\n\n%:\n\tdh $@\n",
        )
        .unwrap();

        let result = run_apply(tmp.path(), true).unwrap();
        assert_eq!(
            result.description,
            "Rely on pre-initialized dpkg-architecture variables."
        );
        let new_content = fs::read_to_string(&rules).unwrap();
        assert!(new_content.contains("include /usr/share/dpkg/architecture.mk"));
        assert!(!new_content.contains("DEB_HOST_ARCH"));
    }

    #[test]
    fn test_no_matching_variables_opinionated() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#! /usr/bin/make -f\n\nFOO := bar\n\n%:\n\tdh $@\n",
        )
        .unwrap();
        assert!(matches!(
            run_apply(tmp.path(), true),
            Err(FixerError::NoChanges)
        ));
    }
}
