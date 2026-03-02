use crate::{FixerError, FixerResult, LintianIssue};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let old_path = base_path.join("debian/source.lintian-overrides");

    if !old_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let issue = LintianIssue::source_with_info(
        "old-source-override-location",
        vec!["debian/source.lintian-overrides".to_string()],
    );

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    let source_dir = base_path.join("debian/source");
    let new_path = source_dir.join("lintian-overrides");

    // Create debian/source directory if it doesn't exist
    if !source_dir.exists() {
        fs::create_dir_all(&source_dir)?;
    }

    // Read the content of the old file
    let old_content = fs::read_to_string(&old_path)?;

    if new_path.exists() {
        // If the new file already exists, append the content
        let mut new_content = fs::read_to_string(&new_path)?;
        new_content.push_str(&old_content);
        fs::write(&new_path, new_content)?;
    } else {
        // If the new file doesn't exist, move the content
        fs::write(&new_path, old_content)?;
    }

    // Remove the old file
    fs::remove_file(&old_path)?;

    Ok(
        FixerResult::builder("Move source package lintian overrides to debian/source.")
            .fixed_issue(issue)
            .certainty(crate::Certainty::Certain)
            .build(),
    )
}

declare_fixer! {
    name: "package-uses-deprecated-source-override-location",
    tags: ["old-source-override-location"],
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
    fn test_simple_move() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let old_path = debian_dir.join("source.lintian-overrides");
        fs::write(&old_path, "foo source: some-tag exact match\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Move source package lintian overrides to debian/source."
        );
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Old file should be removed
        assert!(!old_path.exists());

        // New file should exist with same content
        let new_path = debian_dir.join("source/lintian-overrides");
        assert!(new_path.exists());
        let content = fs::read_to_string(&new_path).unwrap();
        assert_eq!(content, "foo source: some-tag exact match\n");
    }

    #[test]
    fn test_append_to_existing() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let old_path = debian_dir.join("source.lintian-overrides");
        let new_path = source_dir.join("lintian-overrides");

        fs::write(&old_path, "foo source: some-tag exact match\n").unwrap();
        fs::write(&new_path, "bar source: another-tag\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Old file should be removed
        assert!(!old_path.exists());

        // New file should have both contents
        let content = fs::read_to_string(&new_path).unwrap();
        assert_eq!(
            content,
            "bar source: another-tag\nfoo source: some-tag exact match\n"
        );
    }

    #[test]
    fn test_no_old_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
