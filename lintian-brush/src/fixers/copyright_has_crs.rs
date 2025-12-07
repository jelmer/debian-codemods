use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");

    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Read the file as bytes to preserve exact binary content
    let content = fs::read(&copyright_path)?;

    // Check if there are any CR characters
    if !content.contains(&b'\r') {
        return Err(FixerError::NoChanges);
    }

    // Create issue and check if it should be fixed
    let issue = LintianIssue::source("copyright-has-crs");

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Remove all CR characters
    let new_content: Vec<u8> = content.into_iter().filter(|&b| b != b'\r').collect();

    // Write back the modified content
    fs::write(&copyright_path, new_content)?;

    Ok(FixerResult::builder("Remove CRs from copyright file.")
        .fixed_issue(issue)
        .certainty(crate::Certainty::Certain)
        .build())
}

declare_fixer! {
    name: "copyright-has-crs",
    tags: ["copyright-has-crs"],
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
    fn test_remove_crs() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        // Write content with CRLF line endings
        fs::write(
            &copyright_path,
            b"Format: example\r\nUpstream-Name: test\r\n\r\nFiles: *\r\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove CRs from copyright file.");
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Verify LintianIssue was created correctly
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(result.fixed_lintian_issues[0].tag, Some("copyright-has-crs".to_string()));
        assert_eq!(result.fixed_lintian_issues[0].info, None);

        // Check that CRs were removed
        let content = fs::read(&copyright_path).unwrap();
        assert!(!content.contains(&b'\r'));
        assert_eq!(
            content,
            b"Format: example\nUpstream-Name: test\n\nFiles: *\n"
        );
    }

    #[test]
    fn test_no_crs() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        // Write content with LF line endings only
        fs::write(&copyright_path, b"Format: example\nUpstream-Name: test\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_copyright_file() {
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
