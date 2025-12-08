use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use patchkit::quilt::{Series, SeriesEntry};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

fn mentioned_in_comments(series: &Series) -> HashSet<String> {
    let mut mentioned = HashSet::new();

    for entry in series.iter() {
        if let SeriesEntry::Comment(comment) = entry {
            // Extract potential patch names from comments
            // Handle formats like "# patchname" or "# patchname.patch"
            let comment = comment.trim_start_matches('#').trim();
            // Split on whitespace and take the first word as potential patch name
            if let Some(word) = comment.split_whitespace().next() {
                mentioned.insert(word.to_string());
            }
        }
    }

    mentioned
}

pub fn run(base_path: &Path, opinionated: bool) -> Result<FixerResult, FixerError> {
    // In a lot of cases, it seems like removing the patch is not the right
    // thing to do.
    if !opinionated {
        return Err(FixerError::NoChanges);
    }

    let series_path = base_path.join("debian/patches/series");
    let patches_dir = base_path.join("debian/patches");

    // Read the series file
    let series = match fs::File::open(&series_path) {
        Ok(file) => Series::read(file)
            .map_err(|e| FixerError::Other(format!("Failed to read series file: {}", e)))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(FixerError::NoChanges);
        }
        Err(e) => return Err(e.into()),
    };

    // Get patches mentioned in comments (commented-out patches)
    let commented_out = mentioned_in_comments(&series);

    // Check if patches directory exists
    if !patches_dir.is_dir() {
        return Err(FixerError::NoChanges);
    }

    // Find patches not in series
    let mut removed = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for entry in fs::read_dir(&patches_dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        // Only process regular files
        if !path.is_file() {
            continue;
        }

        // Don't delete the series file or 00list
        if name == "series" || name == "00list" {
            continue;
        }

        // Ignore any README files
        if name.starts_with("README") {
            continue;
        }

        // Skip if patch is listed in series
        if series.contains(name.as_ref()) {
            continue;
        }

        // Skip if patch is mentioned in comments (commented-out)
        if commented_out.contains(name.as_ref()) {
            continue;
        }

        // Create issue for this patch
        let issue = LintianIssue::source_with_info(
            "patch-file-present-but-not-mentioned-in-series",
            vec![format!("[debian/patches/{}]", name)],
        );

        // Check if we should fix this issue (not overridden)
        if !issue.should_fix(base_path) {
            overridden_issues.push(issue);
            continue;
        }

        // Remove the patch file
        fs::remove_file(&path)?;
        removed.push(name.to_string());
        fixed_issues.push(issue);
    }

    if removed.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    removed.sort();

    let description = if removed.len() == 1 {
        format!(
            "Remove patch {} that is missing from debian/patches/series",
            removed[0]
        )
    } else {
        format!(
            "Remove patches {} that are missing from debian/patches/series",
            removed.join(", ")
        )
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "patch-file-present-but-not-mentioned-in-series",
    tags: ["patch-file-present-but-not-mentioned-in-series"],
    apply: |basedir, _package, _version, preferences| {
        let opinionated = preferences.opinionated.unwrap_or(false);
        run(basedir, opinionated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_remove_unlisted_patch() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let patches_dir = debian_dir.join("patches");
        fs::create_dir_all(&patches_dir).unwrap();

        // Create series file with only "one" and "two"
        fs::write(patches_dir.join("series"), "one\ntwo\n").unwrap();

        // Create three patch files
        fs::write(patches_dir.join("one"), "").unwrap();
        fs::write(patches_dir.join("two"), "").unwrap();
        fs::write(patches_dir.join("three"), "").unwrap();

        let result = run(temp_dir.path(), true);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        // Check that "three" was removed
        assert!(!patches_dir.join("three").exists());
        assert!(patches_dir.join("one").exists());
        assert!(patches_dir.join("two").exists());

        let result = result.unwrap();
        assert!(result.description.contains("three"));
    }

    #[test]
    fn test_no_changes_when_all_patches_listed() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let patches_dir = debian_dir.join("patches");
        fs::create_dir_all(&patches_dir).unwrap();

        fs::write(patches_dir.join("series"), "one\ntwo\n").unwrap();
        fs::write(patches_dir.join("one"), "").unwrap();
        fs::write(patches_dir.join("two"), "").unwrap();

        let result = run(temp_dir.path(), true);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_not_opinionated() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let patches_dir = debian_dir.join("patches");
        fs::create_dir_all(&patches_dir).unwrap();

        fs::write(patches_dir.join("series"), "one\n").unwrap();
        fs::write(patches_dir.join("one"), "").unwrap();
        fs::write(patches_dir.join("two"), "").unwrap();

        let result = run(temp_dir.path(), false);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Check that "two" was NOT removed
        assert!(patches_dir.join("two").exists());
    }

    #[test]
    fn test_ignores_readme() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let patches_dir = debian_dir.join("patches");
        fs::create_dir_all(&patches_dir).unwrap();

        fs::write(patches_dir.join("series"), "one\n").unwrap();
        fs::write(patches_dir.join("one"), "").unwrap();
        fs::write(patches_dir.join("README"), "").unwrap();
        fs::write(patches_dir.join("README.md"), "").unwrap();

        let result = run(temp_dir.path(), true);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Check that README files were NOT removed
        assert!(patches_dir.join("README").exists());
        assert!(patches_dir.join("README.md").exists());
    }

    #[test]
    fn test_multiple_patches_removed() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let patches_dir = debian_dir.join("patches");
        fs::create_dir_all(&patches_dir).unwrap();

        fs::write(patches_dir.join("series"), "one\n").unwrap();
        fs::write(patches_dir.join("one"), "").unwrap();
        fs::write(patches_dir.join("two"), "").unwrap();
        fs::write(patches_dir.join("three"), "").unwrap();

        let result = run(temp_dir.path(), true);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert!(result.description.contains("patches"));
        assert!(result.description.contains("three"));
        assert!(result.description.contains("two"));
    }
}
