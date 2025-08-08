/// Simplified integration test for Python package debianization
use breezyshim::tree::{MutableTree, Tree};
use breezyshim::workingtree::WorkingTree;
use debianize::{debianize, DebianizePreferences, SessionPreferences};
use std::path::Path;
use tempfile::TempDir;
use upstream_ontologist::UpstreamMetadata;

#[test]
fn test_debianize_python_simple() {
    // Initialize breezy
    breezyshim::init();

    // Create a temporary directory
    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path();

    // Write files directly to filesystem instead of using VCS
    let setup_py_content = r#"#!/usr/bin/env python3
from setuptools import setup, find_packages

setup(
    name="hello-world",
    version="0.1.0",
    author="Test Author",
    author_email="test@example.com",
    description="A simple hello world package",
    packages=find_packages(),
    python_requires=">=3.8",
    install_requires=["requests>=2.25.0"],
)
"#;
    std::fs::write(repo_path.join("setup.py"), setup_py_content).unwrap();

    // Make setup.py executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(repo_path.join("setup.py"))
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(repo_path.join("setup.py"), perms).unwrap();
    }

    // Create package directory
    std::fs::create_dir(repo_path.join("hello_world")).unwrap();

    let init_content = r#"""Simple hello world package"""
__version__ = "0.1.0"

def hello():
    return "Hello, World!"
"#;
    std::fs::write(repo_path.join("hello_world/__init__.py"), init_content).unwrap();

    // Create README
    std::fs::write(
        repo_path.join("README.md"),
        "# Hello World\n\nA test package.",
    )
    .unwrap();

    // Create LICENSE
    std::fs::write(
        repo_path.join("LICENSE"),
        "MIT License\n\nCopyright (c) 2024 Test Author",
    )
    .unwrap();

    // Workaround for ognibuild bug: create minimal pyproject.toml
    let pyproject_content = r#"[build-system]
requires = ["setuptools", "wheel"]
build-backend = "setuptools.build_meta"
"#;
    std::fs::write(repo_path.join("pyproject.toml"), pyproject_content).unwrap();

    // Initialize a git repository using breezyshim
    use breezyshim::controldir::ControlDirFormat;
    let format = ControlDirFormat::default();
    let transport = breezyshim::transport::get_transport(
        &url::Url::from_directory_path(&repo_path).unwrap(),
        None,
    )
    .unwrap();

    let controldir = format.initialize_on_transport(&transport).unwrap();
    controldir.create_repository(None).unwrap();
    controldir.create_branch(None).unwrap();
    let wt = controldir.create_workingtree().unwrap();

    // Add and commit all files
    wt.add(&[
        Path::new("setup.py"),
        Path::new("hello_world"),
        Path::new("hello_world/__init__.py"),
        Path::new("README.md"),
        Path::new("LICENSE"),
        Path::new("pyproject.toml"),
    ])
    .unwrap();

    wt.build_commit()
        .message("Initial commit")
        .commit()
        .unwrap();

    // Force update to ensure working tree sees all files
    wt.update(None).unwrap();

    // Debug: Check that files are visible in working tree
    assert!(
        wt.has_filename(Path::new("setup.py")),
        "setup.py not found in working tree"
    );
    assert!(
        wt.has_filename(Path::new("hello_world")),
        "hello_world dir not found in working tree"
    );
    assert!(
        wt.has_filename(Path::new("hello_world/__init__.py")),
        "__init__.py not found in working tree"
    );

    // Debug: Check the actual path
    let wt_path = wt.basedir();
    eprintln!("Working tree basedir: {:?}", wt_path);
    assert!(
        wt_path.join("setup.py").exists(),
        "setup.py not found on filesystem at {:?}",
        wt_path.join("setup.py")
    );

    // Set up preferences
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
        committer: Some("Test User <test@example.com>".to_string()),
        upstream_version_kind: breezyshim::debian::VersionKind::Auto,
        debian_revision: "1".to_string(),
        team: None,
        author: Some("Test Packager <packager@example.com>".to_string()),
        compat_level: None,
        check_wnpp: false,
        run_fixers: false,
    };

    let metadata = UpstreamMetadata::new();

    // Run debianize
    eprintln!("Running debianize with working tree at: {:?}", wt.basedir());
    eprintln!("Current directory: {:?}", std::env::current_dir().unwrap());

    let result = debianize(
        &wt,
        Path::new(""),
        Some(&wt.branch()),
        Some(Path::new("")),
        &preferences,
        None,
        &metadata,
    );

    // Check result
    assert!(result.is_ok(), "Debianize failed: {:?}", result.err());
    let debianize_result = result.unwrap();

    // Basic checks
    assert!(wt.has_filename(Path::new("debian")));
    assert!(wt.has_filename(Path::new("debian/control")));
    assert!(wt.has_filename(Path::new("debian/rules")));
    assert!(wt.has_filename(Path::new("debian/changelog")));

    // Check version detection - should be a snapshot version since no releases found
    assert!(debianize_result.upstream_version.is_some());
    let version = debianize_result.upstream_version.as_ref().unwrap();
    assert!(
        version.starts_with("0+bzr"),
        "Expected snapshot version, got: {}",
        version
    );

    // Read and verify debian/control using standard filesystem operations
    let control_path = repo_path.join("debian/control");
    let control_content = std::fs::read_to_string(&control_path).unwrap();

    // Replace the temporary directory path in Vcs-Bzr with a placeholder for consistent testing
    let control_content = control_content
        .lines()
        .map(|line| {
            if line.starts_with("Vcs-Bzr: ") {
                "Vcs-Bzr: <TEMPDIR>/".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    // Check exact content
    let expected_control = r#"Source: python-hello-world
Maintainer: Test Packager <packager@example.com>
Rules-Requires-Root: no
Standards-Version: 4.7.2.1
Build-Depends: debhelper-compat (= 13), dh-sequence-python3, python3-all, python3-setuptools
Testsuite: autopkgtest-pkg-python
Vcs-Bzr: <TEMPDIR>/

Package: python3-hello-world
Architecture: all
Depends: ${python3:Depends}
"#;
    assert_eq!(control_content, expected_control);

    // Read and verify debian/rules
    let rules_path = repo_path.join("debian/rules");
    let rules_content = std::fs::read_to_string(&rules_path).unwrap();
    let expected_rules = "#!/usr/bin/make -f\n%:\n\tdh $@ --buildsystem=pybuild\n";
    assert_eq!(rules_content, expected_rules);

    // Read and verify debian/source/format
    let format_path = repo_path.join("debian/source/format");
    let format_content = std::fs::read_to_string(&format_path).unwrap();
    assert_eq!(format_content, "3.0 (quilt)\n");
}

#[test]
fn test_exact_control_file_generation() {
    // Initialize breezy
    breezyshim::init();

    // Create a temporary directory
    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path();

    // Create a minimal Python package
    let setup_py = r#"from setuptools import setup
setup(
    name="test-pkg",
    version="1.0.0",
    author="Test Author",
    author_email="test@example.com",
    description="Test package",
    python_requires=">=3.8",
    install_requires=["requests"],
)
"#;
    std::fs::write(repo_path.join("setup.py"), setup_py).unwrap();
    std::fs::create_dir(repo_path.join("test_pkg")).unwrap();
    std::fs::write(
        repo_path.join("test_pkg/__init__.py"),
        "__version__ = '1.0.0'",
    )
    .unwrap();

    // Initialize git repo
    use breezyshim::controldir::ControlDirFormat;
    let format = ControlDirFormat::default();
    let transport = breezyshim::transport::get_transport(
        &url::Url::from_directory_path(&repo_path).unwrap(),
        None,
    )
    .unwrap();

    let controldir = format.initialize_on_transport(&transport).unwrap();
    controldir.create_repository(None).unwrap();
    controldir.create_branch(None).unwrap();
    let wt = controldir.create_workingtree().unwrap();

    wt.add(&[
        Path::new("setup.py"),
        Path::new("test_pkg"),
        Path::new("test_pkg/__init__.py"),
    ])
    .unwrap();

    wt.build_commit()
        .message("Initial commit")
        .commit()
        .unwrap();

    // Minimal preferences
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
        committer: Some("Test User <test@example.com>".to_string()),
        upstream_version_kind: breezyshim::debian::VersionKind::Auto,
        debian_revision: "1".to_string(),
        team: None,
        author: Some("John Doe <john@example.com>".to_string()),
        compat_level: None,
        check_wnpp: false,
        run_fixers: false,
    };

    let metadata = UpstreamMetadata::new();

    // Debug: print paths
    eprintln!("test_exact_control_file_generation:");
    eprintln!("  repo_path: {:?}", repo_path);
    eprintln!("  wt.basedir(): {:?}", wt.basedir());

    // Run debianize
    let result = debianize(
        &wt,
        Path::new(""),
        Some(&wt.branch()),
        Some(Path::new("")),
        &preferences,
        None,
        &metadata,
    );

    assert!(result.is_ok(), "Debianize failed: {:?}", result.err());

    // Read the generated control file
    let control_content = std::fs::read_to_string(repo_path.join("debian/control")).unwrap();

    // Remove any VCS fields for comparison (they might vary)
    let control_content_cleaned = control_content
        .lines()
        .filter(|line| !line.starts_with("Vcs-"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    // Check exact content
    // Note: In test environment without network, python3-requests won't be detected
    let expected_control = r#"Source: python-test-pkg
Maintainer: John Doe <john@example.com>
Rules-Requires-Root: no
Standards-Version: 4.7.2.1
Build-Depends: debhelper-compat (= 13), dh-sequence-python3, python3-all, python3-setuptools
Testsuite: autopkgtest-pkg-python

Package: python3-test-pkg
Architecture: all
Depends: ${python3:Depends}
"#;

    assert_eq!(control_content_cleaned, expected_control);
}
