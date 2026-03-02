use crate::{FixerError, FixerResult, LintianIssue};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let debian_dir = base_path.join("debian");

    if !debian_dir.exists() || !debian_dir.is_dir() {
        return Err(FixerError::NoChanges);
    }

    let mut removed = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Read directory entries
    let entries = fs::read_dir(&debian_dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            if let Some(package_name) = file_name.strip_suffix(".linda-overrides") {
                // Create issue for this package
                let tag_info = format!("usr/share/linda/overrides/{}", package_name);
                let issue = LintianIssue::binary_with_info(
                    package_name,
                    "package-contains-linda-override",
                    vec![tag_info],
                );

                if !issue.should_fix(base_path) {
                    overridden_issues.push(issue);
                    continue;
                }

                // Remove the file
                fs::remove_file(&path)?;
                removed.push(file_name.to_string());
                fixed_issues.push(issue);
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    let description = format!("Remove obsolete linda overrides: {}", removed.join(", "));

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "package-contains-linda-override",
    tags: ["package-contains-linda-override"],
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
    fn test_remove_linda_overrides() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Create test linda-overrides files
        let override1_path = debian_dir.join("libhugs-cabal-bundled.linda-overrides");
        let override2_path = debian_dir.join("test-package.linda-overrides");
        fs::write(
            &override1_path,
            "Tag: extra-license-file\nData: usr/lib/hugs/packages/Cabal/Distribution/License.hs\n",
        )
        .unwrap();
        fs::write(&override2_path, "Tag: some-other-tag\nData: some/path\n").unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        // Check that files were removed
        assert!(!override1_path.exists());
        assert!(!override2_path.exists());

        let result = result.unwrap();
        assert!(result
            .description
            .contains("Remove obsolete linda overrides:"));
        assert!(result
            .description
            .contains("libhugs-cabal-bundled.linda-overrides"));
        assert!(result.description.contains("test-package.linda-overrides"));
    }

    #[test]
    fn test_no_change_when_no_debian_dir() {
        let temp_dir = TempDir::new().unwrap();

        // Apply the fixer
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
    fn test_no_change_when_no_linda_overrides() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Create some other files
        fs::write(debian_dir.join("control"), "").unwrap();
        fs::write(debian_dir.join("rules"), "").unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Check that other files are still there
        assert!(debian_dir.join("control").exists());
        assert!(debian_dir.join("rules").exists());
    }

    #[test]
    fn test_single_linda_override() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let override_path = debian_dir.join("single.linda-overrides");
        fs::write(&override_path, "Tag: test-tag\n").unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        // Check that file was removed
        assert!(!override_path.exists());

        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Remove obsolete linda overrides: single.linda-overrides"
        );
    }

    #[test]
    fn test_empty_debian_dir() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Apply the fixer to empty debian directory
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
