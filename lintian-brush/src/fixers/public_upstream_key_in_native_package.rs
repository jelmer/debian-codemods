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
    // Check if package is native
    if !current_version.is_native() {
        // Not a native package, nothing to do
        return Err(FixerError::NoChanges);
    }

    // Check if we're in opinionated mode
    if !preferences.opinionated.unwrap_or(false) {
        return Err(FixerError::NoChanges);
    }

    let signing_key_path = base_path.join("debian/upstream/signing-key.asc");

    // Check if the signing key file exists
    if !signing_key_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let issue = LintianIssue::source_with_info(
        "public-upstream-key-in-native-package",
        vec!["[debian/upstream/signing-key.asc]".to_string()],
    );

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Remove the signing key file
    fs::remove_file(&signing_key_path)?;

    // Check if debian/upstream directory is now empty and remove it if so
    let upstream_dir = base_path.join("debian/upstream");
    if upstream_dir.exists() && fs::read_dir(&upstream_dir)?.next().is_none() {
        // Directory is empty, remove it
        fs::remove_dir(&upstream_dir)?;
    }

    Ok(
        FixerResult::builder("Remove upstream signing key in native source package")
            .certainty(crate::Certainty::Certain)
            .fixed_issue(issue)
            .build(),
    )
}

declare_fixer! {
    name: "public-upstream-key-in-native-package",
    tags: ["public-upstream-key-in-native-package"],
    apply: |basedir, package, version, preferences| {
        run(basedir, package, version, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_native_package_with_signing_key() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir_all(&upstream_dir).unwrap();

        let signing_key_path = upstream_dir.join("signing-key.asc");
        fs::write(&signing_key_path, "-----BEGIN PGP PUBLIC KEY BLOCK-----\n").unwrap();

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
            "Remove upstream signing key in native source package"
        );
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that the file was removed
        assert!(!signing_key_path.exists());
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

        let signing_key_path = upstream_dir.join("signing-key.asc");
        fs::write(&signing_key_path, "-----BEGIN PGP PUBLIC KEY BLOCK-----\n").unwrap();

        let preferences = FixerPreferences {
            opinionated: Some(false),
            ..Default::default()
        };

        let version = std::str::FromStr::from_str("1.0").unwrap();
        let result = run(base_path, "test", &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Check that the file was not removed
        assert!(signing_key_path.exists());
    }

    #[test]
    fn test_non_native_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir_all(&upstream_dir).unwrap();

        let signing_key_path = upstream_dir.join("signing-key.asc");
        fs::write(&signing_key_path, "-----BEGIN PGP PUBLIC KEY BLOCK-----\n").unwrap();

        let preferences = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };

        let version = std::str::FromStr::from_str("1.0-1").unwrap();
        let result = run(base_path, "test", &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Check that the file was not removed
        assert!(signing_key_path.exists());
    }

    #[test]
    fn test_no_signing_key_file() {
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

        let signing_key_path = upstream_dir.join("signing-key.asc");
        fs::write(&signing_key_path, "-----BEGIN PGP PUBLIC KEY BLOCK-----\n").unwrap();

        // Add another file to the upstream directory
        let other_file = upstream_dir.join("metadata");
        fs::write(&other_file, "Name: test\n").unwrap();

        let preferences = FixerPreferences {
            opinionated: Some(true),
            ..Default::default()
        };

        let version = std::str::FromStr::from_str("1.0").unwrap();
        let result = run(base_path, "test", &version, &preferences);
        assert!(result.is_ok());

        // Check that the signing key file was removed
        assert!(!signing_key_path.exists());
        // Check that the directory was not removed (has other files)
        assert!(upstream_dir.exists());
        assert!(other_file.exists());
    }
}
