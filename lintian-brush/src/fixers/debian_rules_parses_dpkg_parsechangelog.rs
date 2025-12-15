use crate::{FixerError, FixerPreferences, FixerResult, LintianIssue};
use lazy_static::lazy_static;
use makefile_lossless::Makefile;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const PKG_INFO_PATH: &str = "/usr/share/dpkg/pkg-info.mk";

lazy_static! {
    static ref KNOWN_COMMANDS: Vec<(&'static str, &'static str)> = vec![(
        "dpkg-parsechangelog | sed -n -e 's/^Version: //p'",
        "DEB_VERSION"
    ),];
    static ref VAR_RE: Regex = Regex::new(r"([A-Z_]+)\s*([:?]?=)\s*(.*)").unwrap();
    static ref SHELL_RE: Regex = Regex::new(r"\$\(shell\s+(.*)\)").unwrap();
}

fn load_pkg_info_variables() -> HashSet<String> {
    let mut variables = HashSet::new();

    if let Ok(content) = fs::read_to_string(PKG_INFO_PATH) {
        for line in content.lines() {
            let line = line.trim();
            if let Some(caps) = VAR_RE.captures(line) {
                if let Some(var) = caps.get(1) {
                    variables.insert(var.as_str().to_string());
                }
            }
        }
    }

    variables
}

fn check_if_known_command(value: &str) -> Option<String> {
    // Check if this is a $(shell ...) expression
    if let Some(caps) = SHELL_RE.captures(value.trim()) {
        if let Some(cmd) = caps.get(1) {
            let cmd_str = cmd.as_str().trim();
            for (known_cmd, known_var) in KNOWN_COMMANDS.iter() {
                if cmd_str == *known_cmd {
                    return Some(known_var.to_string());
                }
            }
        }
    }
    None
}

pub fn run(base_path: &Path, _preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");
    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&rules_path)?;
    let mut makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    let pkg_info_vars = load_pkg_info_variables();

    // Check if we already have the include
    let include_line = format!("include {}", PKG_INFO_PATH);
    let already_included = makefile.included_files().any(|f| f == PKG_INFO_PATH);

    // Find variables that match pkg-info.mk variables and use known commands
    let mut vars_to_remove = Vec::new();
    let mut needs_include = false;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for var_def in makefile.variable_definitions() {
        if let Some(name) = var_def.name() {
            if let Some(value) = var_def.raw_value() {
                // Check if this variable is defined in pkg-info.mk
                if pkg_info_vars.contains(&name) {
                    // Check if the value matches a known command
                    if check_if_known_command(&value).is_some() {
                        let issue = LintianIssue::source_with_info(
                            "debian-rules-parses-dpkg-parsechangelog",
                            vec![format!("{} [debian/rules]", name)],
                        );
                        if issue.should_fix(base_path) {
                            vars_to_remove.push(name.clone());
                            fixed_issues.push(issue);
                            needs_include = true;
                        } else {
                            overridden_issues.push(issue);
                        }
                    }
                }
            }
        }
    }

    if !needs_include && vars_to_remove.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Now modify the makefile
    if needs_include && !already_included {
        // Add the include at the beginning (after shebang if present)
        let mut new_content = String::new();
        let lines: Vec<&str> = content.lines().collect();
        let mut added_include = false;

        for (i, line) in lines.iter().enumerate() {
            new_content.push_str(line);
            new_content.push('\n');

            // Add include after shebang or first non-empty line
            if !added_include {
                let is_shebang = i == 0 && line.starts_with("#!");
                let is_comment = line.trim().starts_with('#') && !line.starts_with("#!");
                let is_empty = line.trim().is_empty();

                if is_shebang || (!is_comment && !is_empty) {
                    // Look ahead - if next line is empty, add include after it
                    if i + 1 < lines.len() && lines[i + 1].trim().is_empty() {
                        continue;
                    } else if is_shebang && i + 1 < lines.len() {
                        // Add a blank line after shebang if there isn't one
                        if !lines[i + 1].trim().is_empty() {
                            new_content.push('\n');
                        }
                    }
                    new_content.push_str(&include_line);
                    new_content.push('\n');
                    added_include = true;
                }
            }
        }

        // Reparse with the include added
        makefile = Makefile::read_relaxed(new_content.as_bytes())
            .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;
    }

    // Remove the redundant variable definitions
    for var_name in &vars_to_remove {
        for mut var_def in makefile.variable_definitions() {
            if var_def.name().as_deref() == Some(var_name) {
                var_def.remove();
                break;
            }
        }
    }

    // Write the modified makefile
    let new_content = makefile.to_string();
    fs::write(&rules_path, new_content)?;

    Ok(FixerResult::builder("Avoid invoking dpkg-parsechangelog.")
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "debian-rules-parses-dpkg-parsechangelog",
    tags: ["debian-rules-parses-dpkg-parsechangelog"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_no_rules() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_replaces_dpkg_parsechangelog() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let rules_content = r#"#!/usr/bin/make -f

DEB_VERSION := $(shell dpkg-parsechangelog | sed -n -e 's/^Version: //p')
DEB_UPSTREAM_VERSION := $(shell echo $(DEB_VERSION) | cut -d+ -f1)

%:
	dh $@

version:
	echo $(DEB_VERSION)
"#;
        fs::write(debian_dir.join("rules"), rules_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        let new_content = fs::read_to_string(debian_dir.join("rules")).unwrap();
        assert!(new_content.contains("include /usr/share/dpkg/pkg-info.mk"));
        assert!(!new_content.contains("dpkg-parsechangelog"));
        assert!(new_content.contains("DEB_UPSTREAM_VERSION"));
    }
}
