use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let debian_dir = base_path.join("debian");

    if !debian_dir.exists() {
        return Err(FixerError::NoChanges);
    }

    let mut made_changes = false;

    // Step 1: If debian/upstream exists (as a file), move it to debian/upstream-metadata.yaml
    let upstream_file = debian_dir.join("upstream");

    if upstream_file.exists() && upstream_file.is_file() {
        let upstream_metadata_yaml_path = debian_dir.join("upstream-metadata.yaml");
        fs::rename(&upstream_file, &upstream_metadata_yaml_path)?;
        made_changes = true;
    }

    // Step 2: Move metadata files to debian/upstream/ directory
    let upstream_metadata = debian_dir.join("upstream-metadata");
    let upstream_metadata_yaml = debian_dir.join("upstream-metadata.yaml");

    if upstream_metadata.exists() || upstream_metadata_yaml.exists() {
        let upstream_dir = debian_dir.join("upstream");
        let target_metadata = upstream_dir.join("metadata");

        // Create debian/upstream directory if it doesn't exist
        if !upstream_dir.exists() {
            fs::create_dir_all(&upstream_dir)?;
        }

        // Move upstream-metadata if it exists
        if upstream_metadata.exists() {
            fs::rename(&upstream_metadata, &target_metadata)?;
            made_changes = true;
        }
        // Move upstream-metadata.yaml if it exists (this will overwrite if both exist)
        else if upstream_metadata_yaml.exists() {
            fs::rename(&upstream_metadata_yaml, &target_metadata)?;
            made_changes = true;
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    Ok(
        FixerResult::builder("Move upstream metadata to debian/upstream/metadata.")
            .fixed_tags(vec!["debian-upstream-obsolete-path"])
            .certainty(crate::Certainty::Certain)
            .build(),
    )
}

declare_fixer! {
    name: "debian-upstream-obsolete-path",
    tags: ["debian-upstream-obsolete-path"],
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
    fn test_move_upstream_file_to_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create debian/upstream file
        let upstream_file = debian_dir.join("upstream");
        fs::write(
            &upstream_file,
            "Name: test\nRepository: git://example.com/test\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Move upstream metadata to debian/upstream/metadata."
        );
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // The upstream file should have been moved through two steps:
        // 1. debian/upstream -> debian/upstream-metadata.yaml
        // 2. debian/upstream-metadata.yaml -> debian/upstream/metadata
        // So the original file should now be a directory containing metadata
        assert!(upstream_file.exists());
        assert!(upstream_file.is_dir());
        assert!(!debian_dir.join("upstream-metadata.yaml").exists());
        let target_metadata = debian_dir.join("upstream/metadata");
        assert!(target_metadata.exists());
        let content = fs::read_to_string(&target_metadata).unwrap();
        assert_eq!(content, "Name: test\nRepository: git://example.com/test\n");
    }

    #[test]
    fn test_move_upstream_metadata_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create debian/upstream-metadata file
        let upstream_metadata = debian_dir.join("upstream-metadata");
        fs::write(&upstream_metadata, "Name: test2\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that upstream-metadata was moved to upstream/metadata
        assert!(!upstream_metadata.exists());
        let target_metadata = debian_dir.join("upstream/metadata");
        assert!(target_metadata.exists());
        let content = fs::read_to_string(&target_metadata).unwrap();
        assert_eq!(content, "Name: test2\n");
    }

    #[test]
    fn test_move_upstream_metadata_yaml_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create debian/upstream-metadata.yaml file
        let upstream_metadata_yaml = debian_dir.join("upstream-metadata.yaml");
        fs::write(&upstream_metadata_yaml, "Name: test3\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that upstream-metadata.yaml was moved to upstream/metadata
        assert!(!upstream_metadata_yaml.exists());
        let target_metadata = debian_dir.join("upstream/metadata");
        assert!(target_metadata.exists());
        let content = fs::read_to_string(&target_metadata).unwrap();
        assert_eq!(content, "Name: test3\n");
    }

    #[test]
    fn test_no_upstream_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Only create a control file
        fs::write(debian_dir.join("control"), "Source: test\n").unwrap();

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

    #[test]
    fn test_upstream_is_directory() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create debian/upstream as a directory (should not be moved)
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir(&upstream_dir).unwrap();
        fs::write(upstream_dir.join("metadata"), "Name: existing\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Directory should still exist and be unchanged
        assert!(upstream_dir.exists());
        assert!(upstream_dir.is_dir());
    }
}
