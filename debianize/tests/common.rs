/// Common test helpers for debianize tests
use breezyshim::controldir::ControlDirFormat;
use breezyshim::tree::{MutableTree, Tree};
use breezyshim::workingtree::{GenericWorkingTree, WorkingTree};
use debianize::{DebianizePreferences, SessionPreferences};
use std::path::Path;
use tempfile::TempDir;

/// Initialize a git repository with a working tree at the given path
pub fn init_working_tree(path: &Path) -> GenericWorkingTree {
    breezyshim::init();

    let format = ControlDirFormat::default();
    let transport =
        breezyshim::transport::get_transport(&url::Url::from_file_path(path).unwrap(), None)
            .unwrap();

    let controldir = format.initialize_on_transport(&transport).unwrap();
    controldir.create_repository(None).unwrap();
    controldir.create_branch(None).unwrap();
    controldir.create_workingtree().unwrap()
}

/// Create a simple Python package in the working tree
pub fn create_simple_python_package(
    wt: &GenericWorkingTree,
    name: &str,
    version: &str,
    dependencies: &[&str],
) {
    let module_name = name.replace('-', "_");

    // Create setup.py
    let deps_str = if dependencies.is_empty() {
        String::new()
    } else {
        format!(
            "\n    install_requires=[{}],",
            dependencies
                .iter()
                .map(|d| format!("\"{}\"", d))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    let setup_py = format!(
        r#"#!/usr/bin/env python3
from setuptools import setup, find_packages

setup(
    name="{}",
    version="{}",
    author="Test Author",
    author_email="test@example.com",
    description="Test package for {}",
    packages=find_packages(),
    python_requires=">=3.8",{}
)
"#,
        name, version, name, deps_str
    );

    wt.put_file_bytes_non_atomic(Path::new("setup.py"), setup_py.as_bytes())
        .unwrap();

    // Create package directory
    wt.mkdir(Path::new(&module_name)).unwrap();

    // Create __init__.py
    let init_py = format!(
        r#""""Test package {}"""

__version__ = "{}"

def hello():
    return "Hello from {}"
"#,
        name, version, name
    );

    wt.put_file_bytes_non_atomic(
        Path::new(&format!("{}/__init__.py", module_name)),
        init_py.as_bytes(),
    )
    .unwrap();

    // Create README
    let readme = format!("# {}\n\nTest package version {}\n", name, version);
    wt.put_file_bytes_non_atomic(Path::new("README.md"), readme.as_bytes())
        .unwrap();

    // Add all files
    wt.add(&[
        Path::new("setup.py"),
        Path::new(&module_name),
        Path::new(&format!("{}/__init__.py", module_name)),
        Path::new("README.md"),
    ])
    .unwrap();

    // Commit
    wt.build_commit()
        .message(&format!("Initial commit for {}", name))
        .commit()
        .unwrap();
}

/// Create default debianize preferences for testing
pub fn default_test_preferences() -> DebianizePreferences {
    DebianizePreferences {
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
        session: SessionPreferences::default_isolated(),
        create_dist: None,
        committer: Some("Test User <test@example.com>".to_string()),
        upstream_version_kind: breezyshim::debian::VersionKind::Auto,
        debian_revision: "1".to_string(),
        team: None,
        author: Some("Test Packager <packager@example.com>".to_string()),
        compat_level: None,
        check_wnpp: false,
        run_fixers: false,
    }
}

/// Assert that basic debian files exist in the working tree
pub fn assert_debian_files_exist(wt: &GenericWorkingTree) {
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
    assert!(
        wt.has_filename(Path::new("debian/source/format")),
        "debian/source/format should exist"
    );
}

/// Read and clean control file content (removes VCS fields)
pub fn read_cleaned_control(path: &Path) -> String {
    let content = std::fs::read_to_string(path.join("debian/control")).unwrap();
    content
        .lines()
        .filter(|line| !line.starts_with("Vcs-"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Create a test repository with a simple Python package
pub fn create_test_python_repo(
    temp_dir: &TempDir,
    name: &str,
) -> (std::path::PathBuf, GenericWorkingTree) {
    let repo_path = temp_dir.path().join(name);
    std::fs::create_dir_all(&repo_path).unwrap();
    let wt = init_working_tree(&repo_path);
    (repo_path, wt)
}

#[cfg(test)]
pub(crate) struct DebianImageCached {
    old_env: Option<String>,
}

#[cfg(test)]
impl DebianImageCached {
    pub(crate) fn new() -> Result<Self, ognibuild::session::Error> {
        if let Ok(tarball_path) = std::env::var("OGNIBUILD_DEBIAN_TEST_TARBALL") {
            Ok(DebianImageCached {
                old_env: Some(tarball_path),
            })
        } else if let Ok(tarball_path) =
            ognibuild::session::unshare::cached_debian_tarball_path("sid")
        {
            if tarball_path.exists() {
                let old_env = std::env::var("OGNIBUILD_DEBIAN_TEST_TARBALL").ok();
                std::env::set_var("OGNIBUILD_DEBIAN_TEST_TARBALL", &tarball_path);
                Ok(DebianImageCached { old_env })
            } else {
                eprintln!("Cached Debian tarball does not exist at {:?}", tarball_path);
                Err(ognibuild::session::Error::ImageError(
                    ognibuild::session::ImageError::NoCachedImage,
                ))
            }
        } else {
            Err(ognibuild::session::Error::ImageError(
                ognibuild::session::ImageError::NoCachedImage,
            ))
        }
    }
}

#[cfg(test)]
impl Drop for DebianImageCached {
    fn drop(&mut self) {
        if let Some(old_env) = &self.old_env {
            std::env::set_var("OGNIBUILD_DEBIAN_TEST_TARBALL", old_env);
        } else {
            std::env::remove_var("OGNIBUILD_DEBIAN_TEST_TARBALL");
        }
    }
}
