/// Consolidated tests for recursive debianization functionality
mod common;

use breezyshim::workingtree::WorkingTree;
use common::*;
use debianize::debianize;
use std::path::Path;
use tempfile::TempDir;
use upstream_ontologist::UpstreamMetadata;

#[test]
#[serial_test::serial]
fn test_basic_debianization() {
    let _image_cached = match DebianImageCached::new() {
        Ok(cached) => cached,
        Err(e) => {
            eprintln!("Failed to cache Debian image: {:?}", e);
            return;
        }
    };
    let testenv = breezyshim::testing::TestEnv::new();
    let temp_dir = TempDir::new().unwrap();
    let (repo_path, wt) = create_test_python_repo(&temp_dir, "main-package");

    // Create simple package with no dependencies
    create_simple_python_package(&wt, "main-package", "0.1.0", &[]);

    // Debianize
    let preferences = default_test_preferences();
    let metadata = UpstreamMetadata::new();
    let result = debianize(
        &wt,
        Path::new(""),
        Some(&wt.branch()),
        Some(Path::new("")),
        &preferences,
        Some("0.1.0"),
        &metadata,
    );

    assert!(result.is_ok(), "Debianization failed: {:?}", result.err());
    let result = result.unwrap();
    assert_eq!(result.upstream_version, Some("0.1.0".to_string()));

    // Verify debian files exist
    assert_debian_files_exist(&wt);

    // Verify control file content
    let control_content = read_cleaned_control(&repo_path);
    assert!(control_content.contains("Source: python-main-package"));
    assert!(control_content.contains("Package: python3-main-package"));
    std::mem::drop(testenv);
}
