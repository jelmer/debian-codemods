use breezyshim::tree::Tree;
use breezyshim::workingtree::WorkingTree;
use debianize::DebianizePreferences;
use tempfile::TempDir;
use upstream_ontologist::{
    Certainty, Origin, UpstreamDatum, UpstreamDatumWithMetadata, UpstreamMetadata,
};

#[test]
fn test_go_project_debianization() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-go-project");
    std::fs::create_dir(&project_dir).unwrap();

    // Create go.mod
    std::fs::write(
        project_dir.join("go.mod"),
        r#"module github.com/testuser/test-go-package

go 1.19

require (
	github.com/gorilla/mux v1.8.0
	github.com/sirupsen/logrus v1.9.0
	gopkg.in/yaml.v3 v3.0.1
)

require (
	golang.org/x/sys v0.0.0-20220715151400-c0bba94af5f8 // indirect
)
"#,
    )
    .unwrap();

    // Create simple main.go without external dependencies to avoid hanging
    std::fs::write(
        project_dir.join("main.go"),
        r#"package main

import (
	"encoding/json"
	"fmt"
	"net/http"
)

type Response struct {
	Message string `json:"message"`
	Status  string `json:"status"`
}

func main() {
	// Setup routes
	http.HandleFunc("/", healthHandler)
	http.HandleFunc("/api/status", statusHandler)

	fmt.Println("Starting server on port 8080")
	http.ListenAndServe(":8080", nil)
}

func healthHandler(w http.ResponseWriter, r *http.Request) {
	response := Response{
		Message: "Hello, World!",
		Status:  "healthy",
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(response)
}

func statusHandler(w http.ResponseWriter, r *http.Request) {
	response := Response{
		Message: "Service is running",
		Status:  "ok",
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(response)
}
"#,
    )
    .unwrap();

    // Create a package with utilities
    std::fs::create_dir(project_dir.join("pkg")).unwrap();
    std::fs::create_dir(project_dir.join("pkg/utils")).unwrap();
    std::fs::write(
        project_dir.join("pkg/utils/utils.go"),
        r#"package utils

import (
	"fmt"
	"strings"
)

// FormatName formats a name with proper capitalization
func FormatName(name string) string {
	if name == "" {
		return ""
	}
	return strings.ToUpper(name[:1]) + strings.ToLower(name[1:])
}

// BuildVersion creates a version string
func BuildVersion(major, minor, patch int) string {
	return fmt.Sprintf("v%d.%d.%d", major, minor, patch)
}
"#,
    )
    .unwrap();

    // Create simple test file without external dependencies
    std::fs::write(
        project_dir.join("main_test.go"),
        r#"package main

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestHealthHandler(t *testing.T) {
	req, err := http.NewRequest("GET", "/", nil)
	if err != nil {
		t.Fatal(err)
	}

	rr := httptest.NewRecorder()
	handler := http.HandlerFunc(healthHandler)
	handler.ServeHTTP(rr, req)

	if status := rr.Code; status != http.StatusOK {
		t.Errorf("handler returned wrong status code: got %v want %v",
			status, http.StatusOK)
	}

	var response Response
	if err := json.Unmarshal(rr.Body.Bytes(), &response); err != nil {
		t.Errorf("Failed to parse response: %v", err)
	}

	if response.Status != "healthy" {
		t.Errorf("Expected status 'healthy', got '%s'", response.Status)
	}
}

func TestStatusHandler(t *testing.T) {
	req, err := http.NewRequest("GET", "/api/status", nil)
	if err != nil {
		t.Fatal(err)
	}

	rr := httptest.NewRecorder()
	handler := http.HandlerFunc(statusHandler)
	handler.ServeHTTP(rr, req)

	if status := rr.Code; status != http.StatusOK {
		t.Errorf("handler returned wrong status code: got %v want %v",
			status, http.StatusOK)
	}
}
"#,
    )
    .unwrap();

    // Create README
    std::fs::write(
        project_dir.join("README.md"),
        "# Test Go Package\n\nA test Go package for debianization testing.\n",
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

    // Scope the working tree operations to ensure proper cleanup
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
            datum: UpstreamDatum::Repository(
                "https://github.com/testuser/test-go-package".to_string(),
            ),
            certainty: Some(Certainty::Confident),
            origin: Some(Origin::Other("test".to_string())),
        });

        let preferences = DebianizePreferences {
            net_access: false,
            trust: true,
            check: false, // Disable external checking to prevent hanging
            consult_external_directory: false, // Disable external directory consultation
            force_subprocess: false, // Disable subprocess calls to prevent external tool errors
            session: debianize::SessionPreferences::Plain,
            ..Default::default()
        };

        // Run debianize
        debianize::debianize(
            &wt,
            &subpath,
            Some(&wt.branch()), // use local branch as upstream
            Some(&subpath),     // upstream subpath
            &preferences,
            None, // no upstream version override
            &metadata,
        )
    }; // Working tree dropped here

    match result {
        Ok(debianize_result) => {
            println!("Go debianization successful: {:?}", debianize_result);

            // Re-open the working tree to verify results
            let (wt, subpath) = breezyshim::workingtree::open_containing(&project_dir).unwrap();

            // Verify debian directory was created
            assert!(wt.has_filename(&subpath.join("debian")));
            assert!(wt.has_filename(&subpath.join("debian/control")));
            assert!(wt.has_filename(&subpath.join("debian/rules")));
            assert!(wt.has_filename(&subpath.join("debian/changelog")));

            // Check control file contents
            let control_content = wt.get_file_text(&subpath.join("debian/control")).unwrap();
            let control_str = String::from_utf8_lossy(&control_content);

            // Should follow Go naming conventions (golang- prefix)
            assert!(control_str.contains("Source: golang-github-testuser-test-go-package"));
            assert!(control_str.contains("Package: golang-github-testuser-test-go-package-dev"));

            // Should contain Go-specific metadata
            assert!(control_str.contains("XS-Go-Import-Path: github.com/testuser/test-go-package"));

            // Should be architecture all for Go dev packages
            assert!(control_str.contains("Architecture: all"));

            // Should have Multi-Arch: foreign
            assert!(control_str.contains("Multi-Arch: foreign"));

            // Should contain golang addon
            assert!(control_str.contains("dh-sequence-golang"));

            // Should have Go testsuite
            assert!(control_str.contains("Testsuite: autopkgtest-pkg-go"));

            // Should be in devel section
            assert!(control_str.contains("Section: devel"));

            // Check rules file
            let rules_content = wt.get_file_text(&subpath.join("debian/rules")).unwrap();
            let rules_str = String::from_utf8_lossy(&rules_content);
            assert!(rules_str.contains("dh $@ --buildsystem=golang --builddirectory=_build"));

            // Should exclude examples if they exist
            if project_dir.join("examples").exists() {
                assert!(rules_str.contains("export DH_GOLANG_EXCLUDES=examples/"));
            }
        }
        Err(e) => {
            panic!("Go debianization failed: {:?}", e);
        }
    }
}

#[test]
fn test_go_library_only_project() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-go-lib");
    std::fs::create_dir(&project_dir).unwrap();

    // Create go.mod for library
    std::fs::write(
        project_dir.join("go.mod"),
        r#"module github.com/example/go-utils-lib

go 1.20

require (
	github.com/stretchr/testify v1.8.4
)
"#,
    )
    .unwrap();

    // Create library files without main package
    std::fs::write(
        project_dir.join("math.go"),
        r#"// Package utils provides mathematical utility functions
package utils

import "math"

// Add returns the sum of two integers
func Add(a, b int) int {
	return a + b
}

// Multiply returns the product of two integers
func Multiply(a, b int) int {
	return a * b
}

// Distance calculates the Euclidean distance between two points
func Distance(x1, y1, x2, y2 float64) float64 {
	dx := x2 - x1
	dy := y2 - y1
	return math.Sqrt(dx*dx + dy*dy)
}
"#,
    )
    .unwrap();

    std::fs::write(
        project_dir.join("strings.go"),
        r#"package utils

import (
	"strings"
	"unicode"
)

// Reverse reverses a string
func Reverse(s string) string {
	runes := []rune(s)
	for i, j := 0, len(runes)-1; i < j; i, j = i+1, j-1 {
		runes[i], runes[j] = runes[j], runes[i]
	}
	return string(runes)
}

// IsPalindrome checks if a string is a palindrome
func IsPalindrome(s string) bool {
	s = strings.ToLower(s)
	cleaned := ""
	for _, r := range s {
		if unicode.IsLetter(r) || unicode.IsDigit(r) {
			cleaned += string(r)
		}
	}
	return cleaned == Reverse(cleaned)
}

// Title converts string to title case
func Title(s string) string {
	return strings.Title(strings.ToLower(s))
}
"#,
    )
    .unwrap();

    // Create test file
    std::fs::write(
        project_dir.join("math_test.go"),
        r#"package utils

import (
	"testing"
	"github.com/stretchr/testify/assert"
)

func TestAdd(t *testing.T) {
	assert.Equal(t, 5, Add(2, 3))
	assert.Equal(t, 0, Add(-1, 1))
}

func TestMultiply(t *testing.T) {
	assert.Equal(t, 6, Multiply(2, 3))
	assert.Equal(t, 0, Multiply(0, 5))
}

func TestDistance(t *testing.T) {
	assert.Equal(t, 5.0, Distance(0, 0, 3, 4))
	assert.Equal(t, 0.0, Distance(1, 1, 1, 1))
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

    // Open the working tree
    let (wt, subpath) = breezyshim::workingtree::open_containing(&project_dir).unwrap();

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("go-utils-lib".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Repository("https://github.com/example/go-utils-lib".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        check: false, // Disable external checking to prevent hanging
        consult_external_directory: false, // Disable external directory consultation
        force_subprocess: false, // Disable subprocess calls to prevent external tool errors
        session: debianize::SessionPreferences::Plain,
        ..Default::default()
    };

    let result = debianize::debianize(
        &wt,
        &subpath,
        Some(&wt.branch()),
        Some(&subpath),
        &preferences,
        Some("0.1.0"),
        &metadata,
    );

    assert!(
        result.is_ok(),
        "Go library-only project debianization should succeed"
    );

    // Verify debian files
    assert!(wt.has_filename(&subpath.join("debian/control")));

    let control_content = wt.get_file_text(&subpath.join("debian/control")).unwrap();
    let control_str = String::from_utf8_lossy(&control_content);

    // Should create -dev package for library
    assert!(control_str.contains("-dev"));
    assert!(control_str.contains("golang-github-example-go-utils-lib"));
    assert!(control_str.contains("XS-Go-Import-Path: github.com/example/go-utils-lib"));
}

#[test]
fn test_go_project_with_examples() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-go-with-examples");
    std::fs::create_dir(&project_dir).unwrap();

    // Create go.mod
    std::fs::write(
        project_dir.join("go.mod"),
        r#"module github.com/demo/go-with-examples

go 1.19
"#,
    )
    .unwrap();

    // Create main library
    std::fs::write(
        project_dir.join("demo.go"),
        r#"package demo

// Greet returns a greeting message
func Greet(name string) string {
	if name == "" {
		return "Hello, World!"
	}
	return "Hello, " + name + "!"
}
"#,
    )
    .unwrap();

    // Create examples directory
    std::fs::create_dir(project_dir.join("examples")).unwrap();
    std::fs::write(
        project_dir.join("examples/simple.go"),
        r#"package main

import (
	"fmt"
	"github.com/demo/go-with-examples"
)

func main() {
	fmt.Println(demo.Greet("Go Developer"))
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

    // Open the working tree
    let (wt, subpath) = breezyshim::workingtree::open_containing(&project_dir).unwrap();

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("go-with-examples".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Repository("https://github.com/demo/go-with-examples".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        check: false, // Disable external checking to prevent hanging
        consult_external_directory: false, // Disable external directory consultation
        force_subprocess: false, // Disable subprocess calls to prevent external tool errors
        session: debianize::SessionPreferences::Plain,
        ..Default::default()
    };

    let result = debianize::debianize(
        &wt,
        &subpath,
        Some(&wt.branch()),
        Some(&subpath),
        &preferences,
        Some("0.2.0"),
        &metadata,
    );

    assert!(
        result.is_ok(),
        "Go project with examples should debianize successfully"
    );

    // Check rules file excludes examples
    let rules_content = wt.get_file_text(&subpath.join("debian/rules")).unwrap();
    let rules_str = String::from_utf8_lossy(&rules_content);
    assert!(rules_str.contains("export DH_GOLANG_EXCLUDES=examples/"));
}
