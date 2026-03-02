use crate::{FixerError, FixerResult};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let watch_path = base_path.join("debian/watch");

    if !watch_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&watch_path)?;

    let watch_file = debian_watch::parse::parse(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse watch file: {}", e)))?;

    let mut made_changes = false;

    for mut entry in watch_file.entries() {
        let url = entry.url();

        // Only process GitHub URLs
        if !url.contains("github.com") {
            continue;
        }

        // Get the matching pattern
        let Some(pattern) = entry.matching_pattern() else {
            continue;
        };

        // Check if pattern uses old GitHub archive format
        // Old: .*/archive/something
        // New: .*/archive/refs/tags/something
        if pattern.contains("/archive/") && !pattern.contains("/archive/refs/tags/") {
            // Insert refs/tags/ after /archive/
            let new_pattern = pattern.replace("/archive/", "/archive/refs/tags/");
            entry.set_matching_pattern(&new_pattern);
            made_changes = true;
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    fs::write(&watch_path, watch_file.to_string())?;

    Ok(FixerResult::builder(
        "Update pattern for GitHub archive URLs from /<org>/<repo>/tags page/<org>/<repo>/archive/<tag> → /<org>/<repo>/archive/refs/tags/<tag>."
    )
    .certainty(crate::Certainty::Likely)
    .build())
}

declare_fixer! {
    name: "debian-watch-file-uses-old-github-pattern",
    tags: ["debian-watch-file-uses-old-github-pattern"],
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
    fn test_updates_old_github_pattern() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content =
            "version=4\nhttps://github.com/jupyter/jupyter_core/tags .*/archive/(.*)\\.tar\\.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let result = run(temp_dir.path()).unwrap();
        assert!(result.description.contains("archive"));

        let updated_content = fs::read_to_string(&watch_path).unwrap();
        assert!(updated_content.contains("/archive/refs/tags/"));
        assert!(!updated_content.contains("/archive/("));
    }

    #[test]
    fn test_no_watch_file() {
        let temp_dir = TempDir::new().unwrap();
        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_updated() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content = "version=4\nhttps://github.com/jupyter/jupyter_core/tags .*/archive/refs/tags/(.*)\\.tar\\.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_non_github() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content =
            "version=4\nhttps://example.com/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
