use crate::{FixerError, FixerPreferences, FixerResult, LintianIssue};
use makefile_lossless::Makefile;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    // Check if we're not in opinionated mode and directory only contains "debian"
    if !preferences.opinionated.unwrap_or(false) {
        // Check if directory only contains "debian"
        let entries: Result<Vec<_>, _> =
            fs::read_dir(base_path).map_err(FixerError::from)?.collect();
        let entries = entries.map_err(FixerError::from)?;

        if entries.len() == 1 && entries[0].file_name() == "debian" {
            // See https://salsa.debian.org/debian-ayatana-team/snapd-glib/-/merge_requests/6#note_358358
            return Err(FixerError::NoChanges);
        }
    }

    let rules_path = base_path.join("debian/rules");

    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&rules_path)?;
    let mut makefile: Makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    let mut rule_index: Option<usize> = None;
    let mut certainty = crate::Certainty::Certain;
    let mut fixed_issue: Option<LintianIssue> = None;

    // Find get-orig-source rules
    for (idx, rule) in makefile.rules().enumerate() {
        for target in rule.targets() {
            if target.trim() == "get-orig-source" {
                rule_index = Some(idx);

                // Check if commands are just "uscan"
                let recipes: Vec<String> = rule.recipes().map(|r| r.trim().to_string()).collect();

                if !recipes.is_empty() {
                    // Check if all commands start with "uscan"
                    let all_uscan = recipes
                        .iter()
                        .all(|cmd| cmd.split_whitespace().next() == Some("uscan"));

                    if !all_uscan {
                        certainty = crate::Certainty::Possible;
                    }
                }

                // Check if we should fix this issue
                let issue = LintianIssue::source_with_info(
                    "debian-rules-contains-unnecessary-get-orig-source-target",
                    vec!["[debian/rules]".to_string()],
                );

                if !issue.should_fix(base_path) {
                    return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
                }

                fixed_issue = Some(issue);
                break;
            }
        }
        if rule_index.is_some() {
            break;
        }
    }

    let rule_index = match rule_index {
        Some(idx) => idx,
        None => return Err(FixerError::NoChanges),
    };

    let fixed_issue = fixed_issue.expect("issue should be set when rule found");

    // Remove the get-orig-source rule
    makefile
        .remove_rule(rule_index)
        .map_err(|e| FixerError::Other(format!("Failed to remove rule: {}", e)))?;

    // Remove from .PHONY if present
    // This will remove "get-orig-source" from .PHONY prerequisites,
    // and if .PHONY becomes empty, it will remove the entire .PHONY rule
    makefile
        .remove_phony_target("get-orig-source")
        .map_err(|e| FixerError::Other(format!("Failed to remove phony target: {}", e)))?;

    // Write back the modified makefile
    fs::write(&rules_path, makefile.to_string())?;

    Ok(
        FixerResult::builder("Remove unnecessary get-orig-source-target.")
            .certainty(certainty)
            .fixed_issues(vec![fixed_issue])
            .build(),
    )
}

declare_fixer! {
    name: "debian-rules-contains-unnecessary-get-orig-source-target",
    tags: ["debian-rules-contains-unnecessary-get-orig-source-target"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_removes_get_orig_source() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = r#"#!/usr/bin/make -f

%:
	dh $@

get-orig-source:
	uscan
"#;

        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &crate::FixerPreferences {
                opinionated: Some(true),
                ..Default::default()
            },
        );
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&rules_path).unwrap();
        assert!(!updated_content.contains("get-orig-source"));
    }

    #[test]
    fn test_no_change_when_no_get_orig_source() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = r#"#!/usr/bin/make -f

%:
	dh $@
"#;

        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

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
    fn test_no_change_when_only_debian_dir_not_opinionated() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = r#"#!/usr/bin/make -f

get-orig-source:
	uscan
"#;

        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &crate::FixerPreferences {
                opinionated: Some(false),
                ..Default::default()
            },
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
