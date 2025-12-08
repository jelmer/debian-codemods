use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_control::Priority;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut changed_packages = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Get default priority from source paragraph
    let default_priority = if let Some(source) = editor.source() {
        source.priority().map(|p| p.to_string())
    } else {
        None
    };

    // Process binary packages
    for mut binary in editor.binaries() {
        // Only process packages in libs section
        if binary.section().as_deref() != Some("libs") {
            continue;
        }

        // Get priority (from binary or fall back to source default)
        let priority = binary
            .priority()
            .map(|p| p.to_string())
            .or(default_priority.clone())
            .unwrap_or_default();

        // Check if priority is excessive for library packages
        if matches!(priority.as_str(), "required" | "important" | "standard") {
            if let Some(package_name) = binary.name() {
                let issue = LintianIssue::binary_with_info(
                    package_name,
                    "excessive-priority-for-library-package",
                    vec![priority.clone()],
                );

                if issue.should_fix(base_path) {
                    // Set priority to optional
                    binary.set_priority(Some(Priority::Optional));
                    changed_packages.push(package_name.to_string());
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
            }
        }
    }

    if changed_packages.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    let description = if changed_packages.len() == 1 {
        format!(
            "Set priority for library package {} to optional.",
            changed_packages[0]
        )
    } else {
        format!(
            "Set priority for library packages {} to optional.",
            changed_packages.join(", ")
        )
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "excessive-priority-for-library-package",
    tags: ["excessive-priority-for-library-package"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_simple_library_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: bzip2\nPriority: required\n\nPackage: libbzip2\nSection: libs\nPriority: required\nDescription: blah blah\n blah\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Set priority for library package libbzip2 to optional."
        );

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(content.contains("Priority: optional"));
        assert!(!content.contains("Priority: required\nDescription"));
    }

    #[test]
    fn test_implied_priority_from_source() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: bzip2\nPriority: required\n\nPackage: libbzip2\nSection: libs\nDescription: blah blah\n blah\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Set priority for library package libbzip2 to optional."
        );

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(content.contains("Priority: optional"));
        // Should still have source priority
        assert!(content.contains("Source: bzip2\nPriority: required\n"));
    }

    #[test]
    fn test_multiple_library_packages() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nPriority: standard\n\nPackage: libtest1\nSection: libs\nPriority: important\nDescription: Test 1\n Test\n\nPackage: libtest2\nSection: libs\nDescription: Test 2\n Test\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Set priority for library packages libtest1, libtest2 to optional."
        );

        let content = fs::read_to_string(&control_path).unwrap();
        // Both packages should have optional priority
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines.contains(&"Priority: optional"));
        // Count how many times "Priority: optional" appears
        let optional_count = lines
            .iter()
            .filter(|line| **line == "Priority: optional")
            .count();
        assert_eq!(optional_count, 2);
    }

    #[test]
    fn test_non_library_package_unchanged() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\n\nPackage: test-app\nSection: utils\nPriority: required\nDescription: Test app\n Test\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_library_package_already_optional() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\n\nPackage: libtest\nSection: libs\nPriority: optional\nDescription: Test\n Test\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
