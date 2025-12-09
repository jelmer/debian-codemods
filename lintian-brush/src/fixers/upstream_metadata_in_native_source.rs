use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debversion::Version;
use std::fs;
use std::path::Path;

pub fn run(
    base_path: &Path,
    _package_name: &str,
    current_version: &Version,
    preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    // Check if package is native - debversion::Version should have this method
    if !current_version.is_native() {
        // Not a native package, nothing to do
        return Err(FixerError::NoChanges);
    }

    // Check if we're in opinionated mode
    if !preferences.opinionated.unwrap_or(false) {
        return Err(FixerError::NoChanges);
    }

    let metadata_path = base_path.join("debian/upstream/metadata");

    // Check if the metadata file exists
    if !metadata_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let issue = LintianIssue::source_with_info(
        "upstream-metadata-in-native-source",
        vec!["[debian/upstream/metadata]".to_string()],
    );

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Remove the metadata file
    fs::remove_file(&metadata_path)?;

    // Check if debian/upstream directory is now empty and remove it if so
    let upstream_dir = base_path.join("debian/upstream");
    if upstream_dir.exists() && fs::read_dir(&upstream_dir)?.next().is_none() {
        // Directory is empty, remove it
        fs::remove_dir(&upstream_dir)?;
    }

    Ok(
        FixerResult::builder("Remove debian/upstream/metadata in native source package")
            .certainty(crate::Certainty::Certain)
            .fixed_issue(issue)
            .build(),
    )
}

declare_fixer! {
    name: "upstream-metadata-in-native-source",
    tags: ["upstream-metadata-in-native-source"],
    apply: |basedir, package, version, preferences| {
        run(basedir, package, version, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_native_version() {
        use std::str::FromStr;

        let native_v1 = Version::from_str("1.0").unwrap();
        assert!(native_v1.is_native());

        let native_v2 = Version::from_str("2.5.3").unwrap();
        assert!(native_v2.is_native());

        let non_native_v1 = Version::from_str("1.0-1").unwrap();
        assert!(!non_native_v1.is_native());

        let non_native_v2 = Version::from_str("2.5.3-2ubuntu1").unwrap();
        assert!(!non_native_v2.is_native());
    }

    #[test]
    fn test_native_package_with_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir_all(&upstream_dir).unwrap();

        let metadata_path = upstream_dir.join("metadata");
        fs::write(&metadata_path, "Name: test\n").unwrap();

        let preferences = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };

        let version = std::str::FromStr::from_str("1.0").unwrap();
        let result = run(base_path, "test", &version, &preferences);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Remove debian/upstream/metadata in native source package"
        );
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that the file was removed
        assert!(!metadata_path.exists());
        // Check that the empty directory was removed
        assert!(!upstream_dir.exists());
    }

    #[test]
    fn test_native_package_not_opinionated() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir_all(&upstream_dir).unwrap();

        let metadata_path = upstream_dir.join("metadata");
        fs::write(&metadata_path, "Name: test\n").unwrap();

        let preferences = FixerPreferences {
            opinionated: Some(false),
            ..Default::default()
        };

        let version = std::str::FromStr::from_str("1.0").unwrap();
        let result = run(base_path, "test", &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Check that the file was not removed
        assert!(metadata_path.exists());
    }

    #[test]
    fn test_non_native_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir_all(&upstream_dir).unwrap();

        let metadata_path = upstream_dir.join("metadata");
        fs::write(&metadata_path, "Name: test\n").unwrap();

        let preferences = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };

        let version = std::str::FromStr::from_str("1.0-1").unwrap();
        let result = run(base_path, "test", &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Check that the file was not removed
        assert!(metadata_path.exists());
    }

    #[test]
    fn test_no_metadata_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let preferences = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };

        let version = std::str::FromStr::from_str("1.0").unwrap();
        let result = run(base_path, "test", &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_upstream_dir_with_other_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir_all(&upstream_dir).unwrap();

        let metadata_path = upstream_dir.join("metadata");
        fs::write(&metadata_path, "Name: test\n").unwrap();

        // Add another file to the upstream directory
        let other_file = upstream_dir.join("repository");
        fs::write(&other_file, "https://example.com/repo\n").unwrap();

        let preferences = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };

        let version = std::str::FromStr::from_str("1.0").unwrap();
        let result = run(base_path, "test", &version, &preferences);
        assert!(result.is_ok());

        // Check that the metadata file was removed
        assert!(!metadata_path.exists());
        // Check that the directory was not removed (has other files)
        assert!(upstream_dir.exists());
        assert!(other_file.exists());
    }
}
