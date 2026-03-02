use crate::{FixerError, FixerResult, LintianIssue};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");

    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read(&rules_path)?;

    // Replace $(dir $(_)) with $(dir $(firstword $(MAKEFILE_LIST)))
    let pattern = b"$(dir $(_))";
    let replacement = b"$(dir $(firstword $(MAKEFILE_LIST)))";
    let content_str = content.as_slice();

    // Check if pattern exists before making changes
    if !content_str.windows(pattern.len()).any(|w| w == pattern) {
        return Err(FixerError::NoChanges);
    }

    // Create issue and check if we should fix it
    let issue = LintianIssue::source_with_info(
        "debian-rules-uses-special-shell-variable",
        vec!["[debian/rules]".to_string()],
    );
    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    let mut new_content = Vec::new();
    let mut last_end = 0;
    let mut made_changes = false;

    for i in 0..content_str.len() {
        if i + pattern.len() <= content_str.len() && &content_str[i..i + pattern.len()] == pattern {
            // Found a match
            new_content.extend_from_slice(&content_str[last_end..i]);
            new_content.extend_from_slice(replacement);
            last_end = i + pattern.len();
            made_changes = true;
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    // Add the remaining content
    new_content.extend_from_slice(&content_str[last_end..]);

    fs::write(&rules_path, &new_content)?;

    Ok(
        FixerResult::builder("Avoid using $(_) to discover source package directory.")
            .fixed_issues(vec![issue])
            .build(),
    )
}

declare_fixer! {
    name: "debian-rules-uses-special-shell-variable",
    tags: ["debian-rules-uses-special-shell-variable"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_replace_special_shell_variable() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = b"#!/usr/bin/make -f\n\n%:\n\tdh $*\n\nget-orig-source:\n\tuscan --watchfile=$(dir $(_))/watch\n";
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
        assert!(result.is_ok());

        let updated_content = fs::read(&rules_path).unwrap();
        let updated_str = String::from_utf8_lossy(&updated_content);
        assert!(!updated_str.contains("$(dir $(_))"));
        assert!(updated_str.contains("$(dir $(firstword $(MAKEFILE_LIST)))"));
    }

    #[test]
    fn test_no_change_when_not_present() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = b"#!/usr/bin/make -f\n\n%:\n\tdh $*\n";
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
    fn test_no_change_when_no_file() {
        let temp_dir = TempDir::new().unwrap();

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
}
