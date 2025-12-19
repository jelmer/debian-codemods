use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use lazy_static::lazy_static;
use makefile_lossless::{Makefile, MakefileItem};
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const ARCHITECTURE_MK_PATH: &str = "/usr/share/dpkg/architecture.mk";

lazy_static! {
    static ref DPKG_ARCH_VARIABLES: HashSet<String> = {
        let mut vars = HashSet::new();
        // These are the variables defined in /usr/share/dpkg/architecture.mk
        // as per the foreach loops for BUILD, HOST, TARGET machines
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

/// Check if a variable value matches the standard dpkg-architecture call
fn is_standard_dpkg_arch_call(variable_name: &str, value: &str) -> bool {
    if let Some(caps) = DPKG_ARCH_CALL_REGEX.captures(value.trim()) {
        &caps[1] == variable_name
    } else {
        false
    }
}

pub fn run(base_path: &Path, opinionated: bool) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");

    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&rules_path)?;
    let makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    // Check if architecture.mk is already included
    let already_included = makefile.included_files().any(|f| f == ARCHITECTURE_MK_PATH);

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut vars_to_remove = Vec::new();
    let mut vars_to_soften = Vec::new();
    let mut first_different_var_name: Option<String> = None;

    // Find variables that match dpkg architecture variables
    for var_def in makefile.variable_definitions() {
        if let Some(name) = var_def.name() {
            if !DPKG_ARCH_VARIABLES.contains(&name) {
                continue;
            }

            if let Some(value) = var_def.raw_value() {
                // Check if the value matches the standard dpkg-architecture call
                if !is_standard_dpkg_arch_call(&name, &value) {
                    // Value is different - track the first one for include placement
                    if first_different_var_name.is_none() && opinionated {
                        first_different_var_name = Some(name.clone());
                    }
                    continue;
                }

                let assignment_op = var_def.assignment_operator();
                let is_hard = assignment_op.as_deref() != Some("?=");

                // Get line number (1-indexed)
                let line_num = var_def.line() + 1;
                let issue = LintianIssue::source_with_info(
                    "debian-rules-sets-dpkg-architecture-variable",
                    vec![format!("{} [debian/rules:{}]", name, line_num)],
                );

                if !issue.should_fix(base_path) {
                    overridden_issues.push(issue);
                    continue;
                }

                if opinionated {
                    // In opinionated mode, remove all matching lines (both hard and soft)
                    if is_hard {
                        fixed_issues.push(issue);
                    }
                    vars_to_remove.push(name.clone());
                } else {
                    // In non-opinionated mode, only fix hard assignments
                    if is_hard {
                        fixed_issues.push(issue);
                        if already_included {
                            // Include is already present, remove the variable
                            vars_to_remove.push(name.clone());
                        } else {
                            // Change to soft assignment
                            vars_to_soften.push(name.clone());
                        }
                    }
                }
            }
        }
    }

    // In non-opinionated mode, always succeed with the message even if no changes
    // This matches the Python implementation behavior
    let has_changes = !vars_to_remove.is_empty() || !vars_to_soften.is_empty();

    if !has_changes {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        if opinionated {
            return Err(FixerError::NoChanges);
        }
        // Non-opinionated mode: report success with message even if no changes
        return Ok(
            FixerResult::builder("Use ?= for assignments to architecture variables.").build(),
        );
    }

    // Add the include if needed (when removing variables)
    let needs_include = !vars_to_remove.is_empty();

    // Soften variables (change := or = to ?=)
    for var_name in &vars_to_soften {
        for mut var_def in makefile.variable_definitions() {
            if var_def.name().as_deref() == Some(var_name) {
                var_def.set_assignment_operator("?=");
                break;
            }
        }
    }

    // In opinionated mode, replace the first removed variable with the include
    // Create the include item to use for replacement
    let include_item = if needs_include && !already_included {
        let temp_makefile = format!("include {}\n", ARCHITECTURE_MK_PATH)
            .parse::<Makefile>()
            .map_err(|e| FixerError::Other(format!("Failed to create include: {}", e)))?;
        Some(temp_makefile.includes().next().ok_or_else(|| {
            FixerError::Other("Failed to get include from temp makefile".to_string())
        })?)
    } else {
        None
    };

    // Handle include placement and variable removal/replacement
    let mut replaced_first = false;

    // First pass: insert include before first different variable if it exists
    if let (Some(include), Some(target_name)) =
        (include_item.as_ref(), first_different_var_name.as_ref())
    {
        let items: Vec<_> = makefile.items().collect();
        for mut item in items {
            if let MakefileItem::Variable(var) = &item {
                if var.name().as_deref() == Some(target_name.as_str()) {
                    // Insert include before this variable
                    item.insert_before(MakefileItem::Include(include.clone()))
                        .map_err(|e| {
                            FixerError::Other(format!("Failed to insert include: {}", e))
                        })?;
                    replaced_first = true;
                    break;
                }
            }
        }
    }

    // Second pass: remove variables or replace first one with include
    let items: Vec<_> = makefile.items().collect();
    for mut item in items {
        if let MakefileItem::Variable(var) = &item {
            if let Some(name) = var.name() {
                if vars_to_remove.contains(&name) {
                    if !replaced_first && include_item.is_some() {
                        // Replace the first variable with the include
                        item.replace(MakefileItem::Include(include_item.clone().unwrap()))
                            .map_err(|e| {
                                FixerError::Other(format!("Failed to replace variable: {}", e))
                            })?;
                        replaced_first = true;
                    } else {
                        // Remove the variable
                        if let MakefileItem::Variable(mut var) = item {
                            var.remove();
                        }
                    }
                }
            }
        }
    }

    // Write the modified content back
    let result_content = makefile.to_string();
    fs::write(&rules_path, result_content)?;

    let message = if opinionated {
        "Rely on pre-initialized dpkg-architecture variables."
    } else if !vars_to_remove.is_empty() && already_included {
        "Rely on existing architecture.mk include."
    } else {
        "Use ?= for assignments to architecture variables."
    };

    Ok(FixerResult::builder(message)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "debian-rules-sets-dpkg-architecture-variable",
    tags: ["debian-rules-sets-dpkg-architecture-variable"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences.opinionated.unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_content = r#"#! /usr/bin/make -f

DEB_HOST_ARCH := $(shell dpkg-architecture -qDEB_HOST_ARCH)

%:
	dh $@
"#;
        fs::write(debian_dir.join("rules"), rules_content).unwrap();

        let result = run(base_path, false).unwrap();
        assert_eq!(
            result.description,
            "Use ?= for assignments to architecture variables."
        );

        let new_content = fs::read_to_string(debian_dir.join("rules")).unwrap();
        assert!(new_content.contains("DEB_HOST_ARCH ?= $(shell dpkg-architecture -qDEB_HOST_ARCH)"));
    }

    #[test]
    fn test_opinionated_removes_line() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_content = r#"#! /usr/bin/make -f

DEB_HOST_ARCH := $(shell dpkg-architecture -qDEB_HOST_ARCH)

%:
	dh $@
"#;
        fs::write(debian_dir.join("rules"), rules_content).unwrap();

        let result = run(base_path, true).unwrap();
        assert_eq!(
            result.description,
            "Rely on pre-initialized dpkg-architecture variables."
        );

        let new_content = fs::read_to_string(debian_dir.join("rules")).unwrap();
        assert!(new_content.contains("include /usr/share/dpkg/architecture.mk"));
        assert!(!new_content.contains("DEB_HOST_ARCH"));
    }

    #[test]
    fn test_no_matching_variables_non_opinionated() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_content = r#"#! /usr/bin/make -f

FOO := bar

%:
	dh $@
"#;
        fs::write(debian_dir.join("rules"), rules_content).unwrap();

        // Non-opinionated mode always succeeds with message
        let result = run(base_path, false).unwrap();
        assert_eq!(
            result.description,
            "Use ?= for assignments to architecture variables."
        );
    }

    #[test]
    fn test_no_matching_variables_opinionated() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_content = r#"#! /usr/bin/make -f

FOO := bar

%:
	dh $@
"#;
        fs::write(debian_dir.join("rules"), rules_content).unwrap();

        // Opinionated mode returns NoChanges
        let result = run(base_path, true);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
