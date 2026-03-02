use crate::{FixerError, FixerResult, LintianIssue, PackageType};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let options_path = base_path.join("debian/source/options");

    if !options_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&options_path)?;
    let oldlines: Vec<&str> = content.lines().collect();

    let mut dropped: HashSet<String> = HashSet::new();
    let mut newlines: Vec<String> = Vec::new();
    let mut fixed_issues = Vec::new();

    for (lineno, line) in oldlines.iter().enumerate() {
        // Keep comment lines initially
        if line.trim_start().starts_with('#') {
            newlines.push(line.to_string());
            continue;
        }

        // Try to split on '='
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim();

            match key {
                "compression" => {
                    let issue = LintianIssue {
                        package: None,
                        package_type: Some(PackageType::Source),
                        tag: Some("custom-compression-in-debian-source-options".to_string()),
                        info: Some(format!("{} (line {})", line, lineno + 1)),
                    };

                    if !issue.should_fix(base_path) {
                        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
                    }

                    // Drop prior comments
                    while !newlines.is_empty()
                        && newlines.last().unwrap().trim_start().starts_with('#')
                    {
                        newlines.pop();
                    }

                    dropped.insert("custom source compression".to_string());
                    fixed_issues.push(issue);
                    continue;
                }
                "compression-level" => {
                    let issue = LintianIssue {
                        package: None,
                        package_type: Some(PackageType::Source),
                        tag: Some("custom-compression-in-debian-source-options".to_string()),
                        info: Some(format!("{} (line {})", line, lineno + 1)),
                    };

                    if !issue.should_fix(base_path) {
                        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
                    }

                    // Drop prior comments
                    while !newlines.is_empty()
                        && newlines.last().unwrap().trim_start().starts_with('#')
                    {
                        newlines.pop();
                    }

                    dropped.insert("custom source compression level".to_string());
                    fixed_issues.push(issue);
                    continue;
                }
                _ => {}
            }
        }

        newlines.push(line.to_string());
    }

    if dropped.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Write back or delete the file
    if !newlines.is_empty() {
        let new_content = newlines.join("\n") + "\n";
        fs::write(&options_path, new_content)?;
    } else {
        fs::remove_file(&options_path)?;
    }

    let mut sorted_dropped: Vec<_> = dropped.into_iter().collect();
    sorted_dropped.sort();

    Ok(
        FixerResult::builder(format!("Drop {}.", sorted_dropped.join(", ")))
            .fixed_issues(fixed_issues)
            .build(),
    )
}

declare_fixer! {
    name: "debian-source-options-has-custom-compression-settings",
    tags: ["custom-compression-in-debian-source-options"],
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
    fn test_removes_compression() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let options_content = "compression = xz\n";
        let options_path = source_dir.join("options");
        fs::write(&options_path, options_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        // File should be deleted since it's now empty
        assert!(!options_path.exists());
    }

    #[test]
    fn test_removes_compression_level() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let options_content = "compression-level = 9\n";
        let options_path = source_dir.join("options");
        fs::write(&options_path, options_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        assert!(!options_path.exists());
    }

    #[test]
    fn test_keeps_other_options() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let options_content = "compression = xz\nother-option = value\n";
        let options_path = source_dir.join("options");
        fs::write(&options_path, options_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        // File should still exist with other-option
        let updated_content = fs::read_to_string(&options_path).unwrap();
        assert!(updated_content.contains("other-option"));
        assert!(!updated_content.contains("compression"));
    }

    #[test]
    fn test_removes_prior_comments() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let options_content = "# Comment about compression\ncompression = xz\n";
        let options_path = source_dir.join("options");
        fs::write(&options_path, options_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        // Both the comment and the compression line should be removed
        assert!(!options_path.exists());
    }

    #[test]
    fn test_no_change_when_no_custom_compression() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let options_content = "other-option = value\n";
        let options_path = source_dir.join("options");
        fs::write(&options_path, options_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_file() {
        let temp_dir = TempDir::new().unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
