use crate::lintian_overrides::{filter_overrides, LintianOverrides};
use crate::{FixerError, FixerResult, LintianIssue};
use std::fs;
use std::path::{Path, PathBuf};

const REMOVED_TAGS: &[&str] = &[
    "hardening-no-stackprotector",
    "maintainer-not-full-name",
    "uploader-not-full-name",
    "uploader-address-missing",
    "no-upstream-changelog",
    "copyright-year-in-future",
    "script-calls-init-script-directly",
];

// TODO(jelmer): Check if a tag matches a binary package name.

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

fn process_overrides_file(
    path: &Path,
    base_path: &Path,
) -> Result<(bool, Vec<String>, Vec<LintianIssue>, Vec<LintianIssue>), FixerError> {
    let content = fs::read_to_string(path)?;
    let parsed = LintianOverrides::parse(&content);
    let overrides = parsed.ok().map_err(|_| FixerError::NoChanges)?;

    let mut removed_tags = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // First pass: check which lines need fixing and track issues
    for (lineno, line) in overrides.lines().enumerate() {
        if line.is_comment() || line.is_empty() {
            continue;
        }

        if let Some(tag_token) = line.tag() {
            let tag = tag_token.text();
            if REMOVED_TAGS.contains(&tag) {
                let tag_string = tag.to_string();
                if !removed_tags.contains(&tag_string) {
                    removed_tags.push(tag_string.clone());
                }

                // Get package name if specified in the override
                let package_name = line.package_spec().and_then(|spec| spec.package_name());

                // Create issue - if package is specified, it's a binary package override
                let issue = if let Some(pkg) = package_name {
                    LintianIssue::binary_with_info(
                        &pkg,
                        "malformed-override",
                        vec![format!("Unknown tag {} in line {}", tag, lineno + 1)],
                    )
                } else {
                    LintianIssue::source_with_info(
                        "malformed-override",
                        vec![format!("Unknown tag {} in line {}", tag, lineno + 1)],
                    )
                };

                if !issue.should_fix(base_path) {
                    overridden_issues.push(issue);
                } else {
                    fixed_issues.push(issue);
                }
            }
        }
    }

    if fixed_issues.is_empty() && overridden_issues.is_empty() {
        return Ok((false, removed_tags, fixed_issues, overridden_issues));
    }

    // Only make changes if we have issues to fix
    if !fixed_issues.is_empty() {
        // Second pass: filter out the lines
        let filtered = filter_overrides(&overrides, |line| {
            // Always keep comments and empty lines
            if line.is_comment() || line.is_empty() {
                return true;
            }

            // Check if the tag should be removed
            if let Some(tag_token) = line.tag() {
                let tag = tag_token.text();
                if REMOVED_TAGS.contains(&tag) {
                    return false; // Filter out this line
                }
            }

            true // Keep this line
        });

        let new_content = filtered.text();
        if new_content.trim().is_empty() {
            // If the file is now empty, delete it
            fs::remove_file(path)?;
        } else {
            fs::write(path, new_content)?;
        }
    }

    Ok((
        !fixed_issues.is_empty(),
        removed_tags,
        fixed_issues,
        overridden_issues,
    ))
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let override_files = find_override_files(base_path);

    if override_files.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let mut all_removed_tags = Vec::new();
    let mut all_fixed_issues = Vec::new();
    let mut all_overridden_issues = Vec::new();

    for path in override_files {
        match process_overrides_file(&path, base_path) {
            Ok((_, removed_tags, fixed_issues, overridden_issues)) => {
                for tag in removed_tags {
                    if !all_removed_tags.contains(&tag) {
                        all_removed_tags.push(tag);
                    }
                }
                all_fixed_issues.extend(fixed_issues);
                all_overridden_issues.extend(overridden_issues);
            }
            Err(e) => {
                // If it's a not-found or permission error, just skip this file
                if matches!(e, FixerError::Io(_)) {
                    continue;
                } else {
                    return Err(e);
                }
            }
        }
    }

    if all_fixed_issues.is_empty() {
        if !all_overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(all_overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    let description = if !all_removed_tags.is_empty() {
        format!(
            "Remove overrides for lintian tags that are no longer supported: {}",
            all_removed_tags.join(", ")
        )
    } else {
        "Remove overrides for lintian tags that are no longer supported".to_string()
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(all_fixed_issues)
        .overridden_issues(all_overridden_issues)
        .build())
}

declare_fixer! {
    name: "malformed-override",
    tags: ["malformed-override"],
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
    fn test_remove_obsolete_tag() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(
            &overrides_path,
            "lintian-brush source: uploader-not-full-name\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert!(result
            .description
            .contains("Remove overrides for lintian tags that are no longer supported"));
        assert!(result.description.contains("uploader-not-full-name"));
        assert_eq!(result.certainty, None);

        // File should be removed since it's now empty
        assert!(!overrides_path.exists());
    }

    #[test]
    fn test_keep_valid_tag() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(&overrides_path, "some-valid-tag\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // File should still exist with original content
        let content = fs::read_to_string(&overrides_path).unwrap();
        assert_eq!(content, "some-valid-tag\n");
    }

    #[test]
    fn test_mixed_tags() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(
            &overrides_path,
            "valid-tag\nuploader-not-full-name\nanother-valid-tag\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert!(result.description.contains("uploader-not-full-name"));

        // File should exist with only valid tags
        let content = fs::read_to_string(&overrides_path).unwrap();
        assert!(content.contains("valid-tag"));
        assert!(content.contains("another-valid-tag"));
        assert!(!content.contains("uploader-not-full-name"));
    }

    #[test]
    fn test_no_override_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
