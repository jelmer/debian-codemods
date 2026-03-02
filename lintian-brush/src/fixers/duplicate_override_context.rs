use crate::lintian_overrides::{filter_overrides, LintianOverrides};
use crate::{FixerError, FixerResult, LintianIssue};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Find all lintian override files in the package
fn find_override_files(base_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Check debian/source/lintian-overrides
    let source_overrides = base_path.join("debian/source/lintian-overrides");
    if source_overrides.exists() {
        paths.push(source_overrides);
    }

    // Check debian/*.lintian-overrides
    let debian_dir = base_path.join("debian");
    if let Ok(entries) = fs::read_dir(&debian_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name() {
                if name.to_string_lossy().ends_with(".lintian-overrides") {
                    paths.push(path);
                }
            }
        }
    }

    paths
}

/// Process a single overrides file and remove duplicates
fn process_overrides_file(
    path: &Path,
    base_path: &Path,
) -> Result<(bool, Vec<LintianIssue>), FixerError> {
    let content = fs::read_to_string(path)?;
    let parsed = LintianOverrides::parse(&content);
    let overrides = parsed.ok().map_err(|_| FixerError::NoChanges)?;

    // Track line numbers for each override (package, tag, info)
    // Map from (package, tag, info) to list of line numbers
    let mut override_lines: HashMap<(Option<String>, String, String), Vec<usize>> = HashMap::new();
    let mut line_number = 0;

    // First pass: identify duplicates and track line numbers
    for line in overrides.lines() {
        line_number += 1;

        // Skip comments and empty lines
        if line.is_comment() || line.is_empty() {
            continue;
        }

        let package = line.package_spec().and_then(|spec| spec.package_name());
        let tag = line.tag().map(|t| t.text().to_string()).unwrap_or_default();
        let info = line.info().unwrap_or_default();

        let key = (package, tag, info);
        override_lines.entry(key).or_default().push(line_number);
    }

    // Find duplicates (entries with more than one line)
    let duplicates: Vec<_> = override_lines
        .iter()
        .filter(|(_, lines)| lines.len() > 1)
        .collect();

    if duplicates.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Get the relative path for the override file
    let override_file_path = path
        .strip_prefix(base_path.join("debian"))
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    // Sort duplicates by first line number for consistent ordering
    let mut duplicates_sorted = duplicates;
    duplicates_sorted.sort_by_key(|(_, lines)| lines[0]);

    // Check if any of the duplicates should be fixed
    for ((package, tag, override_info), lines) in &duplicates_sorted {
        // Format: "tag override_info (lines X Y) [path]"
        let lines_str = lines
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let info_parts = if override_info.is_empty() {
            vec![
                tag.clone(),
                format!("(lines {})", lines_str),
                format!("[debian/{}]", override_file_path),
            ]
        } else {
            vec![
                tag.clone(),
                override_info.clone(),
                format!("(lines {})", lines_str),
                format!("[debian/{}]", override_file_path),
            ]
        };

        let mut issue = LintianIssue::source_with_info("duplicate-override-context", info_parts);
        issue.package = package.clone();

        if !issue.should_fix(base_path) {
            overridden_issues.push(issue);
        } else {
            fixed_issues.push(issue);
        }
    }

    if fixed_issues.is_empty() {
        return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
    }

    // Second pass: filter out duplicates, keeping only the first occurrence
    let mut seen_overrides = HashMap::new();
    let filtered = filter_overrides(&overrides, |line| {
        // Always keep comments and empty lines
        if line.is_comment() || line.is_empty() {
            return true;
        }

        let package = line.package_spec().and_then(|spec| spec.package_name());
        let tag = line.tag().map(|t| t.text().to_string()).unwrap_or_default();
        let info = line.info().unwrap_or_default();
        let key = (package, tag, info);

        // Keep only the first occurrence using the Entry API
        use std::collections::hash_map::Entry;
        match seen_overrides.entry(key) {
            Entry::Occupied(_) => false,
            Entry::Vacant(entry) => {
                entry.insert(true);
                true
            }
        }
    });

    fs::write(path, filtered.to_string())?;

    Ok((true, fixed_issues))
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let override_files = find_override_files(base_path);

    if override_files.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let mut any_changes = false;
    let mut all_fixed_issues = Vec::new();

    for path in override_files {
        match process_overrides_file(&path, base_path) {
            Ok((changed, fixed_issues)) => {
                if changed {
                    any_changes = true;
                    all_fixed_issues.extend(fixed_issues);
                }
            }
            Err(FixerError::NoChanges) => {
                // No changes needed for this file, continue
            }
            Err(FixerError::NoChangesAfterOverrides(overridden)) => {
                // All duplicates were overridden, but continue checking other files
                return Err(FixerError::NoChangesAfterOverrides(overridden));
            }
            Err(e) => return Err(e),
        }
    }

    if !any_changes {
        return Err(FixerError::NoChanges);
    }

    Ok(FixerResult::builder("Remove duplicate lintian overrides.")
        .fixed_issues(all_fixed_issues)
        .certainty(crate::Certainty::Certain)
        .build())
}

declare_fixer! {
    name: "duplicate-override-context",
    tags: ["duplicate-override-context"],
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
    fn test_duplicate_in_source_overrides() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_content = r#"# Comment
test-package source: some-tag info
test-package source: some-tag info
test-package source: other-tag
"#;
        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(&overrides_path, overrides_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(base_path, "test-package", &version, &Default::default());

        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let content = fs::read_to_string(&overrides_path).unwrap();
        // Should keep only one copy of the duplicate
        assert_eq!(content.matches("some-tag info").count(), 1);
        // Should keep the other non-duplicate tag
        assert!(content.contains("other-tag"));
        // Should keep the comment
        assert!(content.contains("# Comment"));
    }

    #[test]
    fn test_no_duplicates() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_content = r#"test-package source: tag1
test-package source: tag2
"#;
        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(&overrides_path, overrides_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(base_path, "test-package", &version, &Default::default());

        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_duplicates() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_content = r#"pkg source: tag1
pkg source: tag1
pkg source: tag2 info
pkg source: tag2 info
pkg source: tag2 info
"#;
        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(&overrides_path, overrides_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(base_path, "pkg", &version, &Default::default());

        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let content = fs::read_to_string(&overrides_path).unwrap();
        assert_eq!(content.matches("tag1").count(), 1);
        assert_eq!(content.matches("tag2 info").count(), 1);
    }

    #[test]
    fn test_preserves_comments_and_empty_lines() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_content = r#"# Header comment

pkg source: tag1
# Middle comment
pkg source: tag1

"#;
        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(&overrides_path, overrides_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(base_path, "pkg", &version, &Default::default());

        assert!(result.is_ok());

        let content = fs::read_to_string(&overrides_path).unwrap();
        assert!(content.contains("# Header comment"));
        assert!(content.contains("# Middle comment"));
        assert_eq!(content.matches("tag1").count(), 1);
    }
}
