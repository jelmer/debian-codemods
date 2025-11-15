/// Integration tests for debianization
mod common;

use breezyshim::workingtree::WorkingTree;
use common::*;
use debianize::debianize;
use std::path::Path;
use tempfile::TempDir;
use upstream_ontologist::UpstreamMetadata;

#[test]
fn test_debianize_simple_python_package() {
    let temp_dir = TempDir::new().unwrap();
    let (repo_path, wt) = create_test_python_repo(&temp_dir, "hello-world");

    // Create package with dependencies
    create_simple_python_package(&wt, "hello-world", "0.1.0", &["requests>=2.25.0"]);

    // Run debianize
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

    assert!(result.is_ok(), "Debianize failed: {:?}", result.err());

    // Verify debian files exist
    assert_debian_files_exist(&wt);

    // Check debian/control content
    let control_content = read_cleaned_control(&repo_path);
    let expected_control = r#"Source: python-hello-world
Maintainer: Test Packager <packager@example.com>
Build-Depends: debhelper-compat (= 13), dh-sequence-python3, python3-all, python3-setuptools
Standards-Version: 4.7.2.1
Rules-Requires-Root: no
Testsuite: autopkgtest-pkg-python

Package: python3-hello-world
Architecture: all
Depends: ${python3:Depends}
"#;
    assert_eq!(control_content, expected_control);

    // Check debian/rules content
    let rules_path = repo_path.join("debian/rules");
    let rules_content = std::fs::read_to_string(&rules_path).unwrap();
    let expected_rules = "#!/usr/bin/make -f\n%:\n\tdh $@ --buildsystem=pybuild\n";
    assert_eq!(rules_content, expected_rules);

    // Check debian/source/format
    let format_path = repo_path.join("debian/source/format");
    let format_content = std::fs::read_to_string(&format_path).unwrap();
    assert_eq!(format_content, "3.0 (quilt)\n");
}

#[test]
fn test_debianize_with_custom_maintainer() {
    let temp_dir = TempDir::new().unwrap();
    let (repo_path, wt) = create_test_python_repo(&temp_dir, "test-pkg");

    create_simple_python_package(&wt, "test-pkg", "1.0.0", &[]);

    // Custom preferences with different maintainer
    let mut preferences = default_test_preferences();
    preferences.author = Some("John Doe <john@example.com>".to_string());

    let metadata = UpstreamMetadata::new();
    let result = debianize(
        &wt,
        Path::new(""),
        Some(&wt.branch()),
        Some(Path::new("")),
        &preferences,
        Some("1.0.0"),
        &metadata,
    );

    assert!(result.is_ok(), "Debianize failed: {:?}", result.err());

    // Verify custom maintainer in control file
    let control_content = read_cleaned_control(&repo_path);
    assert!(control_content.contains("Maintainer: John Doe <john@example.com>"));
}
