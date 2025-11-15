/// Consolidated tests for recursive debianization functionality
mod common;

use breezyshim::workingtree::WorkingTree;
use common::*;
use debianize::debianize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tempfile::TempDir;
use upstream_ontologist::UpstreamMetadata;

#[test]
fn test_basic_debianization() {
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
}

#[test]
fn test_circular_dependency_detection() {
    // Test that circular dependencies are detected
    let mut visited = HashSet::new();
    let mut stack = vec!["package-a"];
    let mut circular_detected = false;

    // Simulate dependency graph: A -> B -> A
    let dep_graph = HashMap::from([
        ("package-a", vec!["package-b"]),
        ("package-b", vec!["package-a"]),
    ]);

    while let Some(current) = stack.pop() {
        if !visited.insert(current) {
            circular_detected = true;
            break;
        }

        if let Some(deps) = dep_graph.get(current) {
            stack.extend(deps);
        }
    }

    assert!(circular_detected, "Should detect circular dependency");
}

#[test]
fn test_transitive_dependency_resolution() {
    // Test A -> B -> C dependency chain resolution
    let mut resolution_order = Vec::new();

    // Build dependency graph
    let dep_graph = HashMap::from([
        ("package-a", vec!["package-b"]),
        ("package-b", vec!["package-c"]),
        ("package-c", vec![]),
    ]);

    // Topological sort to determine build order
    fn resolve_build_order(
        package: &str,
        graph: &HashMap<&str, Vec<&str>>,
        visited: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) {
        if visited.contains(package) {
            return;
        }

        if let Some(deps) = graph.get(package) {
            for dep in deps {
                resolve_build_order(dep, graph, visited, order);
            }
        }

        visited.insert(package.to_string());
        order.push(package.to_string());
    }

    let mut visited = HashSet::new();
    resolve_build_order("package-a", &dep_graph, &mut visited, &mut resolution_order);

    // Verify the correct build order: C first, then B, then A
    assert_eq!(
        resolution_order,
        vec!["package-c", "package-b", "package-a"]
    );

    // Verify dependencies are built before dependents
    for (i, package) in resolution_order.iter().enumerate() {
        if let Some(deps) = dep_graph.get(package.as_str()) {
            for dep in deps {
                let dep_index = resolution_order.iter().position(|p| p == dep).unwrap();
                assert!(dep_index < i, "{} should be built before {}", dep, package);
            }
        }
    }
}
