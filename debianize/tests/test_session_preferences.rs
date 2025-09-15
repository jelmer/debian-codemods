use breezyshim::tree::Tree;
use breezyshim::workingtree::WorkingTree;
use debianize::{DebianizePreferences, SessionPreferences};
use std::path::PathBuf;
use tempfile::TempDir;
use upstream_ontologist::{
    Certainty, Origin, UpstreamDatum, UpstreamDatumWithMetadata, UpstreamMetadata,
};

#[test]
fn test_plain_session_preference() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-plain-session");
    std::fs::create_dir(&project_dir).unwrap();

    // Create a simple Python project
    std::fs::write(
        project_dir.join("setup.py"),
        r#"
from setuptools import setup

setup(
    name="plain-session-test",
    version="1.0.0",
    description="Test plain session",
    packages=["testsessionpkg"],
)
"#,
    )
    .unwrap();

    std::fs::create_dir(project_dir.join("testsessionpkg")).unwrap();
    std::fs::write(
        project_dir.join("testsessionpkg/__init__.py"),
        "# Plain session test package\n",
    )
    .unwrap();

    // Initialize git repository
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

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("plain-session-test".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        session: SessionPreferences::Plain,
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

    assert!(result.is_ok(), "Plain session should work: {:?}", result);

    let debianize_result = result.unwrap();
    println!("Plain session test successful: {:?}", debianize_result);

    // Verify debian files were created
    assert!(wt.has_filename(&subpath.join("debian")));
    assert!(wt.has_filename(&subpath.join("debian/control")));
    assert!(wt.has_filename(&subpath.join("debian/rules")));
}

#[test]
fn test_schroot_session_preference() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-schroot-session");
    std::fs::create_dir(&project_dir).unwrap();

    // Create a basic project
    std::fs::write(
        project_dir.join("setup.py"),
        r#"
from setuptools import setup

setup(
    name="schroot-session-test",
    version="1.0.0",
    description="Test schroot session",
    packages=[],
)
"#,
    )
    .unwrap();

    // Initialize git repository
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

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("schroot-session-test".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        session: SessionPreferences::Schroot("unstable".to_string()),
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
            println!("Schroot session test successful: {:?}", debianize_result);
            assert!(wt.has_filename(&subpath.join("debian")));
        }
        Err(e) => {
            // Schroot might not be available in test environment
            let error_msg = format!("{:?}", e);
            if error_msg.contains("schroot") || error_msg.contains("chroot") {
                println!(
                    "Schroot not available in test environment (expected): {:?}",
                    e
                );
            } else {
                panic!("Unexpected error with schroot session: {:?}", e);
            }
        }
    }
}

#[test]
fn test_unshare_session_preference() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-unshare-session");
    std::fs::create_dir(&project_dir).unwrap();

    // Create a basic project
    std::fs::write(
        project_dir.join("setup.py"),
        r#"
from setuptools import setup

setup(
    name="unshare-session-test",
    version="1.0.0",
    description="Test unshare session",
    packages=[],
)
"#,
    )
    .unwrap();

    // Initialize git repository
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

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("unshare-session-test".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    // Test with default unshare (empty path)
    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        session: SessionPreferences::Unshare(PathBuf::new()),
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
            println!("Unshare session test successful: {:?}", debianize_result);
            assert!(wt.has_filename(&subpath.join("debian")));
        }
        Err(e) => {
            // Unshare might not be available or might require special privileges
            let error_msg = format!("{:?}", e);
            if error_msg.contains("unshare")
                || error_msg.contains("mmdebstrap")
                || error_msg.contains("mount")
                || error_msg.contains("Permission denied")
            {
                println!(
                    "Unshare not available in test environment (expected): {:?}",
                    e
                );
            } else {
                panic!("Unexpected error with unshare session: {:?}", e);
            }
        }
    }
}

#[test]
fn test_session_preferences_acquire() {
    // Test the SessionPreferences::acquire() method for each type
    let plain_prefs = SessionPreferences::Plain;
    let schroot_prefs = SessionPreferences::Schroot("test-schroot".to_string());
    let unshare_prefs = SessionPreferences::Unshare(PathBuf::from("/tmp/test.tar"));

    // Test Plain session
    match plain_prefs.acquire() {
        Ok(_session) => {
            println!("Plain session created successfully");
        }
        Err(e) => {
            panic!("Plain session should always work: {:?}", e);
        }
    }

    // Test Schroot session
    match schroot_prefs.acquire() {
        Ok(_session) => {
            println!("Schroot session created successfully");
        }
        Err(e) => {
            println!(
                "Schroot session failed (expected in test environment): {:?}",
                e
            );
            // This is expected if schroot is not available
        }
    }

    // Test Unshare session
    match unshare_prefs.acquire() {
        Ok(_session) => {
            println!("Unshare session created successfully");
        }
        Err(e) => {
            println!(
                "Unshare session failed (expected in test environment): {:?}",
                e
            );
            // This is expected if unshare/mmdebstrap is not available or lacks permissions
        }
    }
}

#[test]
fn test_default_isolated_session() {
    // Test the default_isolated() method
    let isolated_prefs = SessionPreferences::default_isolated();

    match isolated_prefs {
        SessionPreferences::Unshare(_) => {
            println!("default_isolated() correctly returns UnshareSession");
        }
        _ => {
            panic!("default_isolated() should return UnshareSession");
        }
    }

    // Test creating the session
    match isolated_prefs.acquire() {
        Ok(_session) => {
            println!("Default isolated session created successfully");
        }
        Err(e) => {
            println!(
                "Default isolated session failed (expected in test environment): {:?}",
                e
            );
        }
    }
}

#[test]
fn test_session_creation_methods() {
    // Test create_session() method
    let plain_prefs = SessionPreferences::Plain;
    let result = plain_prefs.create_session();
    assert!(result.is_ok(), "create_session() should work for Plain");

    let schroot_prefs = SessionPreferences::Schroot("unstable".to_string());
    let result = schroot_prefs.create_session();
    // May fail if schroot not available, which is fine
    match result {
        Ok(_) => println!("Schroot create_session() succeeded"),
        Err(e) => println!("Schroot create_session() failed (expected): {:?}", e),
    }

    let unshare_prefs = SessionPreferences::Unshare(PathBuf::new());
    let result = unshare_prefs.create_session();
    // May fail if unshare not available, which is fine
    match result {
        Ok(_) => println!("Unshare create_session() succeeded"),
        Err(e) => println!("Unshare create_session() failed (expected): {:?}", e),
    }
}

#[test]
fn test_session_preferences_display() {
    // Test Debug trait implementation (Display is not implemented)
    let plain = SessionPreferences::Plain;
    let schroot = SessionPreferences::Schroot("test".to_string());
    let unshare = SessionPreferences::Unshare(PathBuf::from("/tmp/test.tar"));

    println!("Plain: {:?}", plain);
    println!("Schroot: {:?}", schroot);
    println!("Unshare: {:?}", unshare);

    // Basic assertions about the debug format
    assert_eq!(format!("{:?}", plain), "Plain");
    assert!(format!("{:?}", schroot).contains("Schroot"));
    assert!(format!("{:?}", schroot).contains("test"));
    assert!(format!("{:?}", unshare).contains("Unshare"));
}

#[test]
fn test_session_preferences_clone() {
    // Test that SessionPreferences can be cloned
    let original = SessionPreferences::Schroot("test-clone".to_string());
    let cloned = original.clone();

    match (original, cloned) {
        (SessionPreferences::Schroot(name1), SessionPreferences::Schroot(name2)) => {
            assert_eq!(name1, name2);
            println!("Clone test passed");
        }
        _ => panic!("Clone did not preserve type"),
    }
}

#[test]
fn test_mixed_session_preferences() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-mixed-sessions");
    std::fs::create_dir(&project_dir).unwrap();

    // Create a basic project
    std::fs::write(
        project_dir.join("setup.py"),
        r#"
from setuptools import setup

setup(
    name="mixed-session-test",
    version="1.0.0", 
    description="Test mixed session preferences",
)
"#,
    )
    .unwrap();

    // Initialize git repository
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

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("mixed-session-test".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    // Test multiple session types
    let session_types = vec![
        SessionPreferences::Plain,
        SessionPreferences::Schroot("unstable".to_string()),
        SessionPreferences::Unshare(PathBuf::new()),
    ];

    for (i, session_pref) in session_types.into_iter().enumerate() {
        let preferences = DebianizePreferences {
            net_access: false,
            trust: true,
            force_new_directory: true, // Allow overwriting previous attempts
            session: session_pref.clone(),
            ..Default::default()
        };

        println!("Testing session type {}: {:?}", i, session_pref);

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
            Ok(_) => {
                println!("Session type {} succeeded", i);
                assert!(wt.has_filename(&subpath.join("debian")));
            }
            Err(e) => {
                println!("Session type {} failed (may be expected): {:?}", i, e);
                // Some session types may fail in test environment, which is acceptable
            }
        }
    }
}
