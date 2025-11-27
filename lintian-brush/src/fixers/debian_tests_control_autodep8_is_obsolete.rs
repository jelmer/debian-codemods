use crate::{declare_fixer, FixerError, FixerResult, LintianIssue, PackageType};
use std::fs;
use std::path::Path;

const OLD_PATH: &str = "debian/tests/control.autodep8";
const NEW_PATH: &str = "debian/tests/control";

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let old_path = base_path.join(OLD_PATH);
    let new_path = base_path.join(NEW_PATH);

    if !old_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let issue = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        tag: Some("debian-tests-control-autodep8-is-obsolete".to_string()),
        info: Some(vec![OLD_PATH.to_string()]),
    };

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    if !new_path.exists() {
        // Simple case: just rename
        fs::rename(&old_path, &new_path)?;

        Ok(FixerResult::builder(format!(
            "Rename obsolete path {} to {}.",
            OLD_PATH, NEW_PATH
        ))
        .fixed_issue(issue)
        .build())
    } else {
        // Need to merge the files
        let merge_issue = LintianIssue {
            package: None,
            package_type: Some(PackageType::Source),
            tag: Some("debian-tests-control-and-control-autodep8".to_string()),
            info: Some(vec![format!("{} {}", OLD_PATH, NEW_PATH)]),
        };

        if !merge_issue.should_fix(base_path) {
            return Err(FixerError::NoChangesAfterOverrides(vec![merge_issue]));
        }

        // Read the old file
        let old_content = fs::read(&old_path)?;

        // Append to the new file
        let mut new_content = fs::read(&new_path)?;
        new_content.push(b'\n');
        new_content.extend_from_slice(&old_content);

        fs::write(&new_path, new_content)?;
        fs::remove_file(&old_path)?;

        Ok(
            FixerResult::builder(format!("Merge {} into {}.", OLD_PATH, NEW_PATH))
                .fixed_issues(vec![merge_issue, issue])
                .build(),
        )
    }
}

declare_fixer! {
    name: "debian-tests-control-autodep8-is-obsolete",
    tags: ["debian-tests-control-autodep8-is-obsolete", "debian-tests-control-and-control-autodep8"],
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
    fn test_renames_autodep8_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let old_content = "Test-Command: echo test\n";
        let old_path = tests_dir.join("control.autodep8");
        fs::write(&old_path, old_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        // Old file should be gone
        assert!(!old_path.exists());

        // New file should exist with the same content
        let new_path = tests_dir.join("control");
        assert!(new_path.exists());
        let new_content = fs::read_to_string(&new_path).unwrap();
        assert_eq!(new_content, old_content);
    }

    #[test]
    fn test_merges_when_both_exist() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let old_content = "Test-Command: echo old\n";
        let old_path = tests_dir.join("control.autodep8");
        fs::write(&old_path, old_content).unwrap();

        let new_content = "Test-Command: echo new\n";
        let new_path = tests_dir.join("control");
        fs::write(&new_path, new_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        // Old file should be gone
        assert!(!old_path.exists());

        // New file should have both contents
        let merged_content = fs::read_to_string(&new_path).unwrap();
        assert!(merged_content.contains("echo new"));
        assert!(merged_content.contains("echo old"));
    }

    #[test]
    fn test_no_change_when_no_autodep8() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let new_content = "Test-Command: echo test\n";
        let new_path = tests_dir.join("control");
        fs::write(&new_path, new_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_tests_dir() {
        let temp_dir = TempDir::new().unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
