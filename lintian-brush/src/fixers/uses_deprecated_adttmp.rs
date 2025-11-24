use crate::{declare_fixer, FixerError, FixerResult};
use regex::bytes::Regex;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let tests_dir = base_path.join("debian/tests");

    if !tests_dir.is_dir() {
        return Err(FixerError::NoChanges);
    }

    let pattern = Regex::new(r"\bADTTMP\b").unwrap();
    let replacement = b"AUTOPKGTEST_TMP";
    let mut made_changes = false;

    // Iterate through files in debian/tests
    let entries = fs::read_dir(&tests_dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Skip if not a file
        if !path.is_file() {
            continue;
        }

        let content = fs::read(&path)?;
        let new_content = pattern.replace_all(&content, replacement);

        if new_content != content {
            fs::write(&path, new_content.as_ref())?;
            made_changes = true;
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    Ok(
        FixerResult::builder("Replace use of deprecated $ADTTMP with $AUTOPKGTEST_TMP.")
            .certainty(crate::Certainty::Certain)
            .fixed_tags(vec!["uses-deprecated-adttmp"])
            .build(),
    )
}

declare_fixer! {
    name: "uses-deprecated-adttmp",
    tags: ["uses-deprecated-adttmp"],
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
    fn test_replaces_adttmp() {
        let temp_dir = TempDir::new().unwrap();
        let tests_dir = temp_dir.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let test_file = tests_dir.join("athing");
        fs::write(&test_file, b"#!/bin/sh\n\ntouch $ADTTMP/blah\n").unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let updated_content = fs::read(&test_file).unwrap();
        let updated_str = String::from_utf8_lossy(&updated_content);
        assert!(!updated_str.contains("ADTTMP"));
        assert!(updated_str.contains("AUTOPKGTEST_TMP"));
    }

    #[test]
    fn test_no_change_when_no_adttmp() {
        let temp_dir = TempDir::new().unwrap();
        let tests_dir = temp_dir.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let test_file = tests_dir.join("athing");
        fs::write(&test_file, b"#!/bin/sh\n\ntouch $AUTOPKGTEST_TMP/blah\n").unwrap();

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
    fn test_no_change_when_no_tests_dir() {
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

    #[test]
    fn test_multiple_files() {
        let temp_dir = TempDir::new().unwrap();
        let tests_dir = temp_dir.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let test_file1 = tests_dir.join("test1");
        fs::write(&test_file1, b"echo $ADTTMP\n").unwrap();

        let test_file2 = tests_dir.join("test2");
        fs::write(&test_file2, b"cd $ADTTMP && ls\n").unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let content1 = fs::read_to_string(&test_file1).unwrap();
        assert!(content1.contains("AUTOPKGTEST_TMP"));

        let content2 = fs::read_to_string(&test_file2).unwrap();
        assert!(content2.contains("AUTOPKGTEST_TMP"));
    }
}
