use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let watch_path = base_path.join("debian/watch");

    if !watch_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&watch_path)?;

    let mut watch_file: debian_watch::WatchFile = content
        .parse()
        .map_err(|e| FixerError::Other(format!("Failed to parse watch file: {}", e)))?;

    let mut made_changes = false;

    for mut entry in watch_file.entries() {
        let url = entry.url();

        // Only process GitHub URLs
        if !url.contains("github.com") {
            continue;
        }

        // Check if URL uses /releases and change it to /tags
        if url.contains("/releases") {
            let new_url = url.replace("/releases", "/tags");
            entry.set_url(&new_url);
            made_changes = true;
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    fs::write(&watch_path, watch_file.to_string())?;

    Ok(FixerResult::builder("debian/watch: Use GitHub /tags rather than /releases page.").build())
}

declare_fixer! {
    name: "debian-watch-file-uses-github-releases",
    tags: ["debian-watch-file-uses-github-releases"],
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
    fn test_replaces_releases_with_tags() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content =
            "version=4\nhttps://github.com/jupyter/jupyter_core/releases .*/archive/(.*)\\.tar\\.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let result = run(temp_dir.path()).unwrap();
        assert!(result.description.contains("tags"));

        let updated_content = fs::read_to_string(&watch_path).unwrap();
        assert!(updated_content.contains("/tags"));
        assert!(!updated_content.contains("/releases"));
    }

    #[test]
    fn test_no_watch_file() {
        let temp_dir = TempDir::new().unwrap();
        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_uses_tags() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content =
            "version=4\nhttps://github.com/jupyter/jupyter_core/tags .*/archive/(.*)\\.tar\\.gz\n";
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
