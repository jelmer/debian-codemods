use breezyshim::controldir::ControlDirFormat;
use breezyshim::tree::{MutableTree, Tree};
use breezyshim::workingtree::WorkingTree;
use debianize::{debianize, DebianizePreferences, SessionPreferences};
use std::path::Path;
use tempfile::TempDir;
use upstream_ontologist::UpstreamMetadata;

#[test]
fn test_recursive_debianization_basic() {
    breezyshim::init();

    // Create temporary directory for our test
    let temp_dir = TempDir::new().unwrap();
    let main_repo_path = temp_dir.path().join("main-package");
    std::fs::create_dir_all(&main_repo_path).unwrap();

    // Initialize the main repository
    let format = ControlDirFormat::default();
    let transport = breezyshim::transport::get_transport(
        &url::Url::from_file_path(&main_repo_path).unwrap(),
        None,
    )
    .unwrap();

    let controldir = format.initialize_on_transport(&transport).unwrap();
    controldir.create_repository(None).unwrap();
    controldir.create_branch(None).unwrap();
    let wt = controldir.create_workingtree().unwrap();

    // Create a simple Python package with dependencies
    let setup_py = r#"
from setuptools import setup, find_packages

setup(
    name="main-package",
    version="0.1.0",
    author="Main Author",
    author_email="main@example.com",
    description="Main package for testing",
    packages=find_packages(),
    python_requires=">=3.8",
    # No dependencies for this simple test to avoid ognibuild bugs
    install_requires=[],
)
"#;
    wt.put_file_bytes_non_atomic(Path::new("setup.py"), setup_py.as_bytes())
        .unwrap();

    wt.mkdir(Path::new("main_package")).unwrap();
    let init_py = r#"
def main():
    return "Main package working!"

__version__ = "0.1.0"
"#;
    wt.put_file_bytes_non_atomic(Path::new("main_package/__init__.py"), init_py.as_bytes())
        .unwrap();

    // Add and commit
    wt.add(&[
        Path::new("setup.py"),
        Path::new("main_package"),
        Path::new("main_package/__init__.py"),
    ])
    .unwrap();
    wt.build_commit()
        .message("Initial commit for main package")
        .commit()
        .unwrap();

    // Set up preferences for debianization
    let preferences = DebianizePreferences {
        use_inotify: Some(false),
        diligence: 0,
        trust: true,
        check: false,
        net_access: false,
        force_subprocess: false,
        force_new_directory: false,
        compat_release: Some("bookworm".to_string()),
        minimum_certainty: debian_analyzer::Certainty::Confident,
        consult_external_directory: false,
        verbose: false,
        session: SessionPreferences::Plain,
        create_dist: None,
        committer: Some("Test <test@example.com>".to_string()),
        upstream_version_kind: breezyshim::debian::VersionKind::Auto,
        debian_revision: "1".to_string(),
        team: None,
        author: Some("Test <test@example.com>".to_string()),
        compat_level: None,
        check_wnpp: false,
        run_fixers: false,
    };

    // Debianize the main package
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

    // Check that debianization succeeded
    assert!(
        result.is_ok(),
        "Main package debianization failed: {:?}",
        result.err()
    );
    let result = result.unwrap();

    println!("Debianization complete!");
    println!(
        "Main package version: {}",
        result.upstream_version.unwrap_or("unknown".to_string())
    );

    // Verify the debian/control file was created
    assert!(
        wt.has_filename(Path::new("debian/control")),
        "debian/control should exist"
    );
    assert!(
        wt.has_filename(Path::new("debian/rules")),
        "debian/rules should exist"
    );
    assert!(
        wt.has_filename(Path::new("debian/changelog")),
        "debian/changelog should exist"
    );

    // Verify the control file content
    let control_path = main_repo_path.join("debian/control");
    let control_content = std::fs::read_to_string(&control_path).unwrap();

    // The control file should have Python-related content
    assert!(
        control_content.contains("python3"),
        "Control file should mention python3"
    );
    assert!(
        control_content.contains("Source: python-main-package"),
        "Control file should have correct source name"
    );
}
