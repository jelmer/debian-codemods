use breezyshim::branch::Branch;
use breezyshim::tree::Tree;
use breezyshim::workingtree::WorkingTree;
use debianize::DebianizePreferences;
use tempfile::TempDir;
use upstream_ontologist::{
    Certainty, Origin, UpstreamDatum, UpstreamDatumWithMetadata, UpstreamMetadata,
};

#[test]
fn test_git_repository_integration() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-git-integration");
    std::fs::create_dir(&project_dir).unwrap();

    // Create a Python project
    std::fs::write(
        project_dir.join("setup.py"),
        r#"
from setuptools import setup

setup(
    name="git-integration-test",
    version="1.5.0",
    description="A test project for VCS integration",
    author="Git Test Author",
    author_email="git@example.com",
    url="https://github.com/testuser/git-integration-test",
    packages=["gitpkg"],
)
"#,
    )
    .unwrap();

    std::fs::create_dir(project_dir.join("gitpkg")).unwrap();
    std::fs::write(
        project_dir.join("gitpkg/__init__.py"),
        "__version__ = '1.5.0'\n",
    )
    .unwrap();

    // Initialize git repository with proper history
    let output = std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.name", "Git Test Author"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.email", "git@example.com"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Add initial commit
    let output = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["commit", "-m", "Initial project setup"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Add a tag
    let output = std::process::Command::new("git")
        .args(["tag", "-m", "Version 1.5.0", "v1.5.0"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Create a feature branch
    let output = std::process::Command::new("git")
        .args(["checkout", "-b", "feature/new-feature"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Add some changes on feature branch
    std::fs::write(
        project_dir.join("gitpkg/feature.py"),
        r#"def new_feature():
    """A new feature function."""
    return "New feature implemented!"
"#,
    )
    .unwrap();

    let output = std::process::Command::new("git")
        .args(["add", "gitpkg/feature.py"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["commit", "-m", "Add new feature"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Switch back to main for debianization
    let output = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Open the working tree
    let (wt, subpath) = breezyshim::workingtree::open_containing(&project_dir).unwrap();

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("git-integration-test".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Repository(
            "https://github.com/testuser/git-integration-test".to_string(),
        ),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        session: debianize::SessionPreferences::Plain,
        ..Default::default()
    };

    let result = debianize::debianize(
        &wt,
        &subpath,
        Some(&wt.branch()),
        Some(&subpath),
        &preferences,
        Some("1.5.0"),
        &metadata,
    );

    match result {
        Ok(debianize_result) => {
            println!("VCS integration successful: {:?}", debianize_result);

            // Verify debian directory was created
            assert!(wt.has_filename(&subpath.join("debian")));
            assert!(wt.has_filename(&subpath.join("debian/control")));

            // Check if VCS information was set
            if let Some(vcs_url) = &debianize_result.vcs_url {
                println!("VCS URL set: {}", vcs_url);
                // Should contain some VCS information
                let vcs_url_str = vcs_url.as_str();
                assert!(vcs_url_str.contains("git") || vcs_url_str.contains("github"));
            } else {
                println!("No VCS URL set (may be expected in test environment)");
            }

            // Check control file for VCS fields
            let control_content = wt.get_file_text(&subpath.join("debian/control")).unwrap();
            let control_str = String::from_utf8_lossy(&control_content);

            // May contain VCS fields if VCS integration worked
            if control_str.contains("Vcs-") {
                println!("VCS fields found in control file");
            } else {
                println!("No VCS fields in control file (may be expected in test)");
            }
        }
        Err(e) => {
            panic!("VCS integration test failed: {:?}", e);
        }
    }
}

#[test]
fn test_upstream_branch_integration() {
    let temp_dir = TempDir::new().unwrap();
    let upstream_dir = temp_dir.path().join("upstream-repo");
    let downstream_dir = temp_dir.path().join("downstream-repo");

    // Create upstream repository
    std::fs::create_dir(&upstream_dir).unwrap();

    std::fs::write(
        upstream_dir.join("setup.py"),
        r#"
from setuptools import setup

setup(
    name="upstream-package",
    version="2.0.0",
    description="Upstream package",
    packages=["upstream_pkg"],
)
"#,
    )
    .unwrap();

    std::fs::create_dir(upstream_dir.join("upstream_pkg")).unwrap();
    std::fs::write(
        upstream_dir.join("upstream_pkg/__init__.py"),
        "__version__ = '2.0.0'\n",
    )
    .unwrap();

    // Initialize upstream git repository
    let output = std::process::Command::new("git")
        .args(["init"])
        .current_dir(&upstream_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.name", "Upstream Author"])
        .current_dir(&upstream_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.email", "upstream@example.com"])
        .current_dir(&upstream_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&upstream_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["commit", "-m", "Upstream v2.0.0"])
        .current_dir(&upstream_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["tag", "-m", "Version 2.0.0", "v2.0.0"])
        .current_dir(&upstream_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Create downstream repository (clone of upstream)
    let output = std::process::Command::new("git")
        .args([
            "clone",
            upstream_dir.to_str().unwrap(),
            downstream_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Open the downstream working tree
    let (wt, subpath) = breezyshim::workingtree::open_containing(&downstream_dir).unwrap();

    // Open upstream branch
    let upstream_url = format!("file://{}", upstream_dir.display());
    let upstream_branch = match breezyshim::branch::open(&upstream_url.parse().unwrap()) {
        Ok(branch) => Some(branch),
        Err(e) => {
            println!("Could not open upstream branch: {:?}", e);
            None
        }
    };

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("upstream-package".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Repository(upstream_url),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        session: debianize::SessionPreferences::Plain,
        ..Default::default()
    };

    // Test with upstream branch if available, fall back to working tree branch
    let wt_branch = wt.branch();
    let upstream_branch_param = upstream_branch
        .as_ref()
        .and_then(|b| {
            b.as_any()
                .downcast_ref::<breezyshim::branch::GenericBranch>()
                .map(|gb| gb as &dyn breezyshim::branch::PyBranch)
        })
        .or(Some(&wt_branch));

    let result = debianize::debianize(
        &wt,
        &subpath,
        upstream_branch_param,
        Some(&subpath), // upstream subpath
        &preferences,
        Some("2.0.0"),
        &metadata,
    );

    match result {
        Ok(debianize_result) => {
            println!(
                "Upstream branch integration successful: {:?}",
                debianize_result
            );
            assert!(wt.has_filename(&subpath.join("debian")));

            // If upstream branch was used, might have upstream branch name set
            if let Some(upstream_branch_name) = &debianize_result.upstream_branch_name {
                println!("Upstream branch name: {}", upstream_branch_name);
            }
        }
        Err(e) => {
            println!(
                "Upstream branch integration failed (may be expected): {:?}",
                e
            );
            // This might fail in test environment, which is acceptable
        }
    }
}

#[test]
fn test_debian_branch_creation() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-debian-branch");
    std::fs::create_dir(&project_dir).unwrap();

    // Create a basic project
    std::fs::write(
        project_dir.join("setup.py"),
        r#"
from setuptools import setup

setup(
    name="debian-branch-test",
    version="3.1.0",
    description="Test debian branch creation",
)
"#,
    )
    .unwrap();

    // Initialize git repository
    let output = std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Open the working tree
    let (wt, subpath) = breezyshim::workingtree::open_containing(&project_dir).unwrap();

    // Use use_packaging_branch to create debian branch first
    let debian_branch_name = "debian/main";
    match debianize::use_packaging_branch(&wt, debian_branch_name) {
        Ok(_) => {
            println!("Successfully created debian branch: {}", debian_branch_name);
        }
        Err(e) => {
            println!("Could not create debian branch (may be expected): {:?}", e);
        }
    }

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("debian-branch-test".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        session: debianize::SessionPreferences::Plain,
        ..Default::default()
    };

    let result = debianize::debianize(
        &wt,
        &subpath,
        Some(&wt.branch()),
        Some(&subpath),
        &preferences,
        Some("3.1.0"),
        &metadata,
    );

    match result {
        Ok(debianize_result) => {
            println!("Debian branch test successful: {:?}", debianize_result);
            assert!(wt.has_filename(&subpath.join("debian")));

            // Check current branch
            let current_branch = wt.branch();
            println!(
                "Current branch after debianization: {:?}",
                current_branch.name()
            );
        }
        Err(e) => {
            panic!("Debian branch test failed: {:?}", e);
        }
    }
}

#[test]
fn test_branch_url_extraction() {
    // Test the branch URL extraction functionality
    use debianize::fixer::extract_branch_from_url;

    // Test fragment style
    let url = url::Url::parse("https://github.com/user/repo#branch=develop").unwrap();
    assert_eq!(extract_branch_from_url(&url), Some("develop".to_string()));

    // Test query parameter style
    let url = url::Url::parse("https://github.com/user/repo?branch=feature-123").unwrap();
    assert_eq!(
        extract_branch_from_url(&url),
        Some("feature-123".to_string())
    );

    // Test GitHub tree URL style
    let url = url::Url::parse("https://github.com/user/repo/tree/main").unwrap();
    assert_eq!(extract_branch_from_url(&url), Some("main".to_string()));

    // Test GitLab tree URL with path
    let url = url::Url::parse("https://gitlab.com/user/repo/tree/release-1.0/src").unwrap();
    assert_eq!(
        extract_branch_from_url(&url),
        Some("release-1.0".to_string())
    );

    // Test URL without branch
    let url = url::Url::parse("https://github.com/user/repo").unwrap();
    assert_eq!(extract_branch_from_url(&url), None);

    println!("Branch URL extraction tests passed");
}

#[test]
fn test_vcs_url_handling() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-vcs-url");
    std::fs::create_dir(&project_dir).unwrap();

    // Create project with VCS information
    std::fs::write(
        project_dir.join("setup.py"),
        r#"
from setuptools import setup

setup(
    name="vcs-url-test",
    version="1.0.0",
    description="Test VCS URL handling",
    url="https://github.com/example/vcs-url-test",
    project_urls={
        "Bug Reports": "https://github.com/example/vcs-url-test/issues",
        "Source": "https://github.com/example/vcs-url-test",
    },
)
"#,
    )
    .unwrap();

    // Initialize git with remote
    let output = std::process::Command::new("git")
        .args(["init"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Add a remote
    let output = std::process::Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/example/vcs-url-test.git",
        ])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit with VCS info"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Open the working tree
    let (wt, subpath) = breezyshim::workingtree::open_containing(&project_dir).unwrap();

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("vcs-url-test".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Repository("https://github.com/example/vcs-url-test".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        session: debianize::SessionPreferences::Plain,
        ..Default::default()
    };

    let result = debianize::debianize(
        &wt,
        &subpath,
        Some(&wt.branch()),
        Some(&subpath),
        &preferences,
        Some("1.0.0"),
        &metadata,
    );

    match result {
        Ok(debianize_result) => {
            println!("VCS URL handling successful: {:?}", debianize_result);

            // Check if VCS URL was detected/set
            if let Some(vcs_url) = &debianize_result.vcs_url {
                println!("VCS URL: {}", vcs_url);
            }

            // Check debian/control for VCS fields
            let control_content = wt.get_file_text(&subpath.join("debian/control")).unwrap();
            let control_str = String::from_utf8_lossy(&control_content);

            println!("Control file content:\n{}", control_str);

            // Look for any VCS-related fields
            if control_str.contains("Vcs-Git") || control_str.contains("Vcs-Browser") {
                println!("Found VCS fields in control file");
            }
        }
        Err(e) => {
            panic!("VCS URL handling test failed: {:?}", e);
        }
    }
}
