/// Integration tests for debianization
mod common;

use breezyshim::testing::TestEnv;
use breezyshim::workingtree::WorkingTree;
use common::*;
use debian_analyzer::lintian::latest_standards_version;
use debianize::debianize;
use serial_test::serial;
use std::path::Path;
use tempfile::TempDir;
use upstream_ontologist::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata, UpstreamMetadata};

#[test]
#[serial]
fn test_debianize_simple_python_package() {
    let image_cached = match DebianImageCached::new() {
        Ok(cached) => cached,
        Err(e) => {
            eprintln!("Failed to cache Debian image: {:?}", e);
            return;
        }
    };
    let test_env = TestEnv::new();

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
    let expected_control = format!(
        "Source: python-hello-world\n\
         Maintainer: Test Packager <packager@example.com>\n\
         Build-Depends: debhelper-compat (= 13), dh-sequence-python3, python3-all, python3-setuptools\n\
         Standards-Version: {}\n\
         Rules-Requires-Root: no\n\
         Testsuite: autopkgtest-pkg-python\n\
         \n\
         Package: python3-hello-world\n\
         Architecture: all\n\
         Depends: ${{python3:Depends}}\n",
        latest_standards_version()
    );
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

    std::mem::drop(image_cached);
    std::mem::drop(test_env);
}

#[test]
#[serial]
fn test_debianize_with_custom_maintainer() {
    let image_cached = match DebianImageCached::new() {
        Ok(cached) => cached,
        Err(e) => {
            eprintln!("Failed to cache Debian image: {:?}", e);
            return;
        }
    };
    let test_env = TestEnv::new();
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
    std::mem::drop(test_env);
    std::mem::drop(image_cached);
}

/// Create a simple Module::Build::Tiny Perl package in the working tree
fn create_perl_build_tiny_package(wt: &breezyshim::workingtree::GenericWorkingTree, name: &str) {
    use breezyshim::tree::MutableTree;

    wt.put_file_bytes_non_atomic(
        Path::new("Build.PL"),
        b"use Module::Build::Tiny;\nBuild_PL();\n",
    )
    .unwrap();

    let module_path = format!("lib/{}.pm", name);
    wt.mkdir(Path::new("lib")).unwrap();
    wt.put_file_bytes_non_atomic(
        Path::new(&module_path),
        format!("package {};\n1;\n", name.replace('-', "::")).as_bytes(),
    )
    .unwrap();

    wt.add(&[
        Path::new("Build.PL"),
        Path::new("lib"),
        Path::new(&module_path),
    ])
    .unwrap();

    wt.build_commit()
        .message(&format!("Initial commit for {}", name))
        .commit()
        .unwrap();
}

#[test]
#[serial]
fn test_debianize_perl_build_tiny_package() {
    let image_cached = match DebianImageCached::new() {
        Ok(cached) => cached,
        Err(e) => {
            eprintln!("Failed to cache Debian image: {:?}", e);
            return;
        }
    };
    let test_env = TestEnv::new();

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().join("Foo-Bar");
    std::fs::create_dir_all(&repo_path).unwrap();
    let wt = init_working_tree(&repo_path);

    create_perl_build_tiny_package(&wt, "Foo-Bar");

    let preferences = default_test_preferences();
    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("Foo-Bar".to_string()),
        certainty: Some(Certainty::Certain),
        origin: None,
    });

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

    assert_debian_files_exist(&wt);

    // The Depends field contains substitution variables, which must be set
    // verbatim rather than parsed as relations.
    let control_content = read_cleaned_control(&repo_path);
    let expected_control = format!(
        "Source: libfoo-bar-perl\n\
         Maintainer: Test Packager <packager@example.com>\n\
         Build-Depends: debhelper-compat (= 13), libmodule-build-perl\n\
         Standards-Version: {}\n\
         Rules-Requires-Root: no\n\
         Testsuite: autopkgtest-pkg-perl\n\
         \n\
         Package: libfoo-bar-perl\n\
         Architecture: all\n\
         Depends: ${{perl:Depends}}\n",
        latest_standards_version()
    );
    assert_eq!(control_content, expected_control);

    std::mem::drop(image_cached);
    std::mem::drop(test_env);
}
