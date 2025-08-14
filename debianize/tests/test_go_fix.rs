use breezyshim::tree::Tree;
use breezyshim::workingtree::WorkingTree;
use debianize::DebianizePreferences;
use tempfile::TempDir;
use upstream_ontologist::{Certainty, Origin, UpstreamDatum, UpstreamDatumWithMetadata, UpstreamMetadata};

#[test]
fn test_go_project_debianization_fixed() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-go-project-fixed");
    std::fs::create_dir(&project_dir).unwrap();

    println!("Testing Go project with simplified dependencies...");

    // Create go.mod with fewer dependencies to reduce processing time
    std::fs::write(
        project_dir.join("go.mod"),
        r#"module github.com/testuser/test-go-package

go 1.19

require (
	github.com/gorilla/mux v1.8.0
)
"#,
    )
    .unwrap();

    // Create simple main.go
    std::fs::write(
        project_dir.join("main.go"),
        r#"package main

import (
	"fmt"
	"net/http"
	"github.com/gorilla/mux"
)

func main() {
	r := mux.NewRouter()
	r.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprintf(w, "Hello World!")
	})
	http.ListenAndServe(":8080", r)
}
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

    // Open the working tree in a scope to ensure proper cleanup
    let result = {
        let (wt, subpath) = breezyshim::workingtree::open_containing(&project_dir).unwrap();

        let mut metadata = UpstreamMetadata::new();
        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name("test-go-package".to_string()),
            certainty: Some(Certainty::Confident),
            origin: Some(Origin::Other("test".to_string())),
        });
        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Version("1.2.3".to_string()),
            certainty: Some(Certainty::Confident),
            origin: Some(Origin::Other("test".to_string())),
        });
        metadata.insert(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Repository("https://github.com/testuser/test-go-package".to_string()),
            certainty: Some(Certainty::Confident),
            origin: Some(Origin::Other("test".to_string())),
        });

        let preferences = DebianizePreferences {
            net_access: false,
            trust: true,
            check: false,
            consult_external_directory: false,
            force_subprocess: false,
            session: debianize::SessionPreferences::Plain,
            ..Default::default()
        };

        // Run debianize
        debianize::debianize(
            &wt,
            &subpath,
            Some(&wt.branch()),
            Some(&subpath),
            &preferences,
            None,
            &metadata,
        )
    }; // Working tree should be properly closed here

    match result {
        Ok(debianize_result) => {
            println!("Go debianization successful: {:?}", debianize_result);
            
            // Re-open to verify results (in a separate scope)
            let (wt, subpath) = breezyshim::workingtree::open_containing(&project_dir).unwrap();
            
            // Verify debian directory was created
            assert!(wt.has_filename(&subpath.join("debian")));
            assert!(wt.has_filename(&subpath.join("debian/control")));
            assert!(wt.has_filename(&subpath.join("debian/rules")));
            assert!(wt.has_filename(&subpath.join("debian/changelog")));
        }
        Err(e) => {
            panic!("Go debianization failed: {:?}", e);
        }
    }
}