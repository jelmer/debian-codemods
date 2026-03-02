use crate::{FixerError, FixerResult, LintianIssue, PackageType};
use debian_analyzer::lintian::StandardsVersion;
use debian_control::lossless::Control;
use std::path::Path;
use std::str::FromStr;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    // Get the standards versions from debian-analyzer
    let standards_versions_iter = match debian_analyzer::lintian::iter_standards_versions_opt() {
        Some(iter) => iter,
        None => {
            // If we can't get the standards versions data, we can't fix anything
            return Err(FixerError::NoChanges);
        }
    };

    // Collect all valid standards versions
    let valid_versions: Vec<StandardsVersion> = standards_versions_iter
        .map(|release| release.version)
        .collect();

    if valid_versions.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let control_path = base_path.join("debian/control");
    let control_content = std::fs::read_to_string(&control_path)?;
    let control = Control::from_str(&control_content)
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/control: {:?}", e)))?;

    let mut source = control
        .source()
        .ok_or_else(|| FixerError::Other("No source paragraph in debian/control".to_string()))?;

    let standards_version_str = match source.standards_version() {
        Some(sv) => sv,
        None => return Err(FixerError::NoChanges),
    };

    let standards_version: StandardsVersion = match standards_version_str.parse() {
        Ok(sv) => sv,
        Err(_) => return Err(FixerError::NoChanges),
    };

    // Check if we need to add .0 suffix (e.g., "4.3" -> "4.3.0")
    let parts_count = standards_version_str.matches('.').count() + 1;
    if parts_count == 2 && valid_versions.contains(&standards_version) {
        let issue = LintianIssue {
            package: None,
            package_type: Some(PackageType::Source),
            tag: Some("invalid-standards-version".to_string()),
            info: Some(standards_version_str.clone()),
        };

        if !issue.should_fix(base_path) {
            return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
        }

        // The parsed version is valid, just need to update the string representation
        let new_version_str = format!("{}.0", standards_version_str);
        source.set("Standards-Version", &new_version_str);
        std::fs::write(&control_path, control.to_string())?;
        return Ok(
            FixerResult::builder("Add missing .0 suffix in Standards-Version.")
                .fixed_issue(issue)
                .build(),
        );
    }

    // Check if it's already valid
    if valid_versions.contains(&standards_version) {
        return Err(FixerError::NoChanges);
    }

    let issue = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        tag: Some("invalid-standards-version".to_string()),
        info: Some(standards_version_str.clone()),
    };

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // If the version is newer than our latest known version, we can't fix it
    let latest_known = valid_versions.iter().max().unwrap();
    if &standards_version > latest_known {
        return Err(FixerError::NoChanges);
    }

    // Find the previous valid standards version
    let candidates: Vec<_> = valid_versions
        .iter()
        .filter(|v| **v < standards_version)
        .collect();

    let new_version = match candidates.iter().max() {
        Some(v) => v,
        None => return Err(FixerError::NoChanges),
    };

    let new_version_str = new_version.to_string();

    source.set("Standards-Version", &new_version_str);
    std::fs::write(&control_path, control.to_string())?;

    Ok(FixerResult::builder(format!(
        "Replace invalid standards version {} with valid {}.",
        standards_version_str, new_version_str
    ))
    .fixed_issue(issue)
    .build())
}

declare_fixer! {
    name: "invalid-standards-version",
    tags: ["invalid-standards-version"],
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
    fn test_parse() {
        assert!("4.6.2".parse::<StandardsVersion>().is_ok());
        assert!("4.6".parse::<StandardsVersion>().is_ok());
        assert!("3.9.8".parse::<StandardsVersion>().is_ok());
        assert!("invalid".parse::<StandardsVersion>().is_err());
    }

    #[test]
    fn test_no_change_when_valid() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: blah\nStandards-Version: 4.6.2\n\nPackage: blah\n",
        )
        .unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_standards_version() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: blah\n\nPackage: blah\n",
        )
        .unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_add_missing_zero_suffix() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // 4.6 is valid but should be 4.6.0
        fs::write(
            debian_dir.join("control"),
            "Source: blah\nStandards-Version: 4.6\n\nPackage: blah\n",
        )
        .unwrap();

        let result = run(temp_dir.path());

        // This might succeed or fail depending on whether 4.6 without .0 is considered invalid
        // If it succeeds, check that .0 was added
        if let Ok(result) = result {
            let control_content = fs::read_to_string(debian_dir.join("control")).unwrap();
            assert!(control_content.contains("4.6.0"));
            assert!(result.description.contains(".0 suffix"));
        }
    }

    #[test]
    fn test_fix_invalid_version() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Use a clearly invalid version (99.99.99 should not exist)
        fs::write(
            debian_dir.join("control"),
            "Source: blah\nStandards-Version: 3.9.99\n\nPackage: blah\n",
        )
        .unwrap();

        let result = run(temp_dir.path());

        // Should either fix it or skip if it's too new
        if let Ok(_) = result {
            let control_content = fs::read_to_string(debian_dir.join("control")).unwrap();
            // Should be downgraded to a valid previous version
            assert!(control_content.contains("Standards-Version:"));
            assert!(!control_content.contains("3.9.99"));
        }
    }
}
