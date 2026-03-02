use crate::{FixerError, FixerResult, LintianIssue};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let pyversions_path = base_path.join("debian/pyversions");

    if !pyversions_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&pyversions_path)?;
    let pyversions = content.trim();

    if pyversions.starts_with("2.") {
        let issue = LintianIssue::source_with_info(
            "debian-pyversions-is-obsolete",
            vec!["debian/pyversions".to_string()],
        );

        if !issue.should_fix(base_path) {
            return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
        }

        fs::remove_file(&pyversions_path)?;

        Ok(FixerResult::builder("Remove obsolete debian/pyversions.")
            .fixed_issues(vec![issue])
            .build())
    } else {
        Err(FixerError::NoChanges)
    }
}

declare_fixer! {
    name: "debian-pyversions-is-obsolete",
    tags: ["debian-pyversions-is-obsolete"],
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
    fn test_remove_obsolete_pyversions() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let pyversions_path = debian_dir.join("pyversions");
        fs::write(&pyversions_path, "2.6-\n").unwrap();

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
        assert!(!pyversions_path.exists());

        let result = result.unwrap();
        assert_eq!(result.description, "Remove obsolete debian/pyversions.");
    }

    #[test]
    fn test_no_change_when_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

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
    fn test_no_change_when_not_python2() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let pyversions_path = debian_dir.join("pyversions");
        fs::write(&pyversions_path, "3.6-\n").unwrap();

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

        // Check that file still exists
        assert!(pyversions_path.exists());
    }

    #[test]
    fn test_no_change_when_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let pyversions_path = debian_dir.join("pyversions");
        fs::write(&pyversions_path, "").unwrap();

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

        // Check that file still exists
        assert!(pyversions_path.exists());
    }
}
