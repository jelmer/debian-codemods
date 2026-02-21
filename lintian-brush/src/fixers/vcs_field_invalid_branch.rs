use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::abstract_control::AbstractControlEditor;
use debian_analyzer::control::TemplatedControlEditor;
use debian_analyzer::debcargo::DebcargoEditor;
use debian_control::vcs::ParsedVcs;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::Path;

/// Helper function to get the appropriate control editor for a package
fn get_control_editor(base_path: &Path) -> Result<Box<dyn AbstractControlEditor>, FixerError> {
    let control_path = base_path.join("debian/control");
    let debcargo_path = base_path.join("debian/debcargo.toml");

    if debcargo_path.exists() && !control_path.exists() {
        Ok(Box::new(DebcargoEditor::from_directory(base_path)?))
    } else {
        Ok(Box::new(TemplatedControlEditor::open(&control_path)?))
    }
}

#[cfg(feature = "udd")]
async fn get_branch_from_url(vcs_type: &str, url: &str) -> Result<Option<String>, FixerError> {
    use sqlx::Row;

    let client = debian_analyzer::udd::connect_udd_mirror()
        .await
        .map_err(|e| FixerError::Other(format!("Failed to connect to UDD: {}", e)))?;

    let query = "SELECT branch, status, error FROM vcswatch WHERE url = $1 AND vcs = $2";

    let row = sqlx::query(query)
        .bind(url)
        .bind(vcs_type)
        .fetch_optional(&client)
        .await
        .map_err(|e| FixerError::Other(format!("Failed to query vcswatch: {}", e)))?;

    if let Some(row) = row {
        let status: String = row.get(1);
        if status == "ERROR" {
            let error: String = row.get(2);
            return Err(FixerError::Other(format!(
                "vcswatch URL unusable: {}",
                error
            )));
        }
        let branch: Option<String> = row.get(0);
        Ok(branch)
    } else {
        // Not found in vcswatch
        Ok(None)
    }
}

#[cfg(not(feature = "udd"))]
async fn get_branch_from_url(_vcs_type: &str, _url: &str) -> Result<Option<String>, FixerError> {
    Err(FixerError::NoChanges)
}

/// Get the default branch from a Git repository using dulwich
fn get_default_branch(url: &str, branch: Option<&str>) -> Result<Option<String>, FixerError> {
    Python::attach(|py| {
        // Import dulwich.client
        let dulwich_client = py
            .import("dulwich.client")
            .map_err(|e| FixerError::Other(format!("Failed to import dulwich.client: {}", e)))?;

        // Call get_transport_and_path
        let (client, path): (Py<PyAny>, Py<PyAny>) = dulwich_client
            .call_method1("get_transport_and_path", (url,))
            .map_err(|e| {
                FixerError::Other(format!("Failed to call get_transport_and_path: {}", e))
            })?
            .extract()
            .map_err(|e| FixerError::Other(format!("Failed to extract result: {}", e)))?;

        // Call get_refs which returns LsRemoteResult
        let result = client
            .call_method1(py, "get_refs", (path,))
            .map_err(|e| FixerError::Other(format!("Failed to call get_refs: {}", e)))?;

        // Get the symrefs attribute from the result
        let symrefs_obj = result
            .getattr(py, "symrefs")
            .map_err(|e| FixerError::Other(format!("Failed to get symrefs: {}", e)))?;
        let symrefs = symrefs_obj
            .bind(py)
            .cast::<PyDict>()
            .map_err(|e| FixerError::Other(format!("symrefs is not a dict: {}", e)))?;

        // Determine which ref to look up
        let ref_name: Vec<u8> = if let Some(b) = branch {
            format!("refs/heads/{}", b).into_bytes()
        } else {
            b"HEAD".to_vec()
        };

        // Try to get the symref
        let head = match symrefs.get_item(&ref_name) {
            Ok(Some(head)) => head,
            Ok(None) => return Ok(None),
            Err(_) => return Ok(None),
        };

        // Extract the head as bytes
        let head_bytes: Vec<u8> = head
            .extract()
            .map_err(|e| FixerError::Other(format!("Failed to extract head: {}", e)))?;

        // Check if it starts with refs/heads/
        let prefix = b"refs/heads/";
        if head_bytes.starts_with(prefix) {
            let branch_name = String::from_utf8(head_bytes[prefix.len()..].to_vec())
                .map_err(|e| FixerError::Other(format!("Invalid UTF-8 in branch name: {}", e)))?;
            Ok(Some(branch_name))
        } else {
            // Return as-is if it doesn't start with refs/heads/
            let branch_name = String::from_utf8(head_bytes)
                .map_err(|e| FixerError::Other(format!("Invalid UTF-8 in branch name: {}", e)))?;
            Ok(Some(branch_name))
        }
    })
}

pub async fn run(
    base_path: &Path,
    preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    if preferences
        .minimum_certainty
        .is_some_and(|c| c < debian_analyzer::Certainty::Certain)
    {
        return Err(FixerError::NoChanges);
    }

    let mut editor = get_control_editor(base_path)?;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    if let Some(mut source) = editor.source() {
        if let Some(vcs_git) = source.get_vcs_url("Git") {
            // Parse the VCS URL to extract repo URL, branch, and subpath
            let parsed: ParsedVcs = vcs_git
                .parse()
                .map_err(|e| FixerError::Other(format!("Failed to parse Vcs-Git URL: {}", e)))?;

            let repo_url = &parsed.repo_url;
            let branch = parsed.branch.as_deref();
            let subpath = parsed.subpath.as_deref();

            // Query vcswatch for the branch
            match get_branch_from_url("Git", &vcs_git).await {
                Ok(Some(new_branch)) => {
                    if Some(new_branch.as_str()) != branch {
                        let default_branch = match get_default_branch(repo_url, branch) {
                            Ok(db) => db,
                            Err(e) => {
                                tracing::debug!(
                                    "Failed to get default branch from {}: {}",
                                    repo_url,
                                    e
                                );
                                None
                            }
                        };

                        // Only change if opinionated OR the new branch is different from both
                        // the default branch and the current branch
                        let should_change = preferences.opinionated.unwrap_or(false)
                            || default_branch.as_ref().is_none_or(|db| {
                                new_branch.as_str() != db && Some(db.as_str()) != branch
                            });

                        if should_change {
                            // Build the new VCS URL
                            let new_vcs = ParsedVcs {
                                repo_url: repo_url.clone(),
                                branch: Some(new_branch.clone()),
                                subpath: subpath.map(String::from),
                            };

                            let new_vcs_git = new_vcs.to_string();

                            let issue = LintianIssue::source("vcs-field-invalid-branch");
                            if !issue.should_fix(base_path) {
                                overridden_issues.push(issue);
                            } else {
                                source.set_vcs_url("Git", &new_vcs_git);

                                // Update Vcs-Browser if possible
                                let vcs_browser = debian_analyzer::vcs::determine_browser_url(
                                    "git",
                                    &new_vcs_git,
                                    preferences.net_access,
                                );
                                if let Some(browser_url) = vcs_browser {
                                    source.set_vcs_url("Browser", browser_url.as_ref());
                                }

                                fixed_issues.push(issue);
                            }
                        }
                    }
                }
                Ok(None) => {
                    // Not found in vcswatch, nothing to do
                }
                Err(FixerError::Other(msg)) if msg.starts_with("vcswatch URL unusable") => {
                    // Log the warning but don't fail
                    tracing::debug!("{}", msg);
                }
                Err(e) => return Err(e),
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit();

    Ok(
        FixerResult::builder("Set branch from vcswatch in Vcs-Git URL.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "vcs-field-invalid-branch",
    tags: ["vcs-field-invalid-branch"],
    apply: |basedir, _package, _version, preferences| {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| FixerError::Other(format!("Failed to create runtime: {}", e)))?;
        rt.block_on(run(basedir, preferences))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_no_vcs_git() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run(base_path, &preferences));

        // Should return NoChanges since there's no Vcs-Git field
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_vcs_git_with_branch_parsing() {
        // Test that we can parse a Vcs-Git URL with a branch
        let url = "https://salsa.debian.org/team/package.git -b debian/unstable [subdir]";
        let parsed: ParsedVcs = url.parse().unwrap();

        assert_eq!(parsed.repo_url, "https://salsa.debian.org/team/package.git");
        assert_eq!(parsed.branch.as_deref(), Some("debian/unstable"));
        assert_eq!(parsed.subpath.as_deref(), Some("subdir"));
    }

    #[test]
    fn test_vcs_git_without_branch_parsing() {
        // Test parsing a Vcs-Git URL without a branch
        let url = "https://salsa.debian.org/team/package.git";
        let parsed: ParsedVcs = url.parse().unwrap();

        assert_eq!(parsed.repo_url, "https://salsa.debian.org/team/package.git");
        assert_eq!(parsed.branch, None);
        assert_eq!(parsed.subpath, None);
    }

    #[test]
    fn test_get_default_branch_with_local_repo() {
        // Create a local git repository for testing
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test-repo");
        fs::create_dir(&repo_path).unwrap();

        // Initialize a git repo
        let output = std::process::Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(&repo_path)
            .output();

        if output.is_err() {
            // Skip test if git is not available
            return;
        }

        // Configure git user for commits
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Create a commit
        fs::write(repo_path.join("test.txt"), "test").unwrap();
        std::process::Command::new("git")
            .args(["add", "test.txt"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Test get_default_branch
        let repo_url = format!("file://{}", repo_path.display());
        let result = get_default_branch(&repo_url, None);

        match result {
            Ok(Some(branch)) => {
                assert_eq!(branch, "main");
            }
            Ok(None) => {
                // This might happen if dulwich isn't available
                tracing::debug!("get_default_branch returned None");
            }
            Err(e) => {
                // This is expected if dulwich isn't available or can't access the repo
                tracing::debug!(
                    "get_default_branch failed (expected if dulwich unavailable): {}",
                    e
                );
            }
        }
    }

    #[test]
    fn test_low_certainty_returns_no_changes() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Vcs-Git: https://salsa.debian.org/team/package.git -b old-branch

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let mut preferences = FixerPreferences::default();
        preferences.minimum_certainty = Some(debian_analyzer::Certainty::Possible);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run(base_path, &preferences));

        // Should return NoChanges since minimum_certainty is below Certain
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    #[cfg(not(feature = "udd"))]
    fn test_without_udd_feature() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Vcs-Git: https://salsa.debian.org/team/package.git

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run(base_path, &preferences));

        // Without UDD feature, should return NoChanges
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
