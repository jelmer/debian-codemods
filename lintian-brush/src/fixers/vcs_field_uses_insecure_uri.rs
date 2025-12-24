use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::abstract_control::AbstractSource;
use debian_analyzer::control::TemplatedControlEditor;
use std::collections::BTreeSet;
use std::path::Path;

/// Find a secure VCS URL
async fn find_secure_vcs_url(
    url: &str,
    net_access: bool,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    // Parse the VCS URL into components
    let parsed: debian_control::vcs::ParsedVcs = url.parse()?;

    // Parse the repository URL
    let repo_url = match url::Url::parse(&parsed.repo_url) {
        Ok(u) => u,
        Err(_) => return Ok(None),
    };

    // Find secure repository URL
    let secure_repo_url = upstream_ontologist::vcs::find_secure_repo_url(
        repo_url,
        parsed.branch.as_deref(),
        Some(net_access),
    )
    .await;

    match secure_repo_url {
        Some(secure_url) => {
            // Reconstruct the VCS URL with the secure repository URL
            let result = debian_control::vcs::ParsedVcs {
                repo_url: secure_url.to_string(),
                branch: parsed.branch,
                subpath: parsed.subpath,
            };
            Ok(Some(result.to_string()))
        }
        None => Ok(None),
    }
}

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut fields_changed = BTreeSet::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut lp_note = false;

    let net_access_allowed = preferences.net_access.unwrap_or(false);

    if let Some(mut source) = editor.source() {
        // Check all VCS fields
        let vcs_types = vec![
            "Git", "Browser", "Svn", "Bzr", "Hg", "Cvs", "Arch", "Darcs", "Mtn", "Svk",
        ];

        // Create a tokio runtime for async operations
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| FixerError::Other(format!("Failed to create runtime: {}", e)))?;

        for vcs_type in vcs_types {
            if let Some(url) = source.get_vcs_url(vcs_type) {
                // Check for lp: prefix (Launchpad)
                if url.starts_with("lp:") {
                    lp_note = true;
                }

                // Find secure URL
                let new_value = rt
                    .block_on(find_secure_vcs_url(&url, net_access_allowed))
                    .map_err(|e| FixerError::Other(format!("Failed to find secure URL: {}", e)))?;

                if let Some(new_url) = new_value {
                    if new_url != url {
                        let field_name = format!("Vcs-{}", vcs_type);
                        let issue = LintianIssue::source_with_info(
                            "vcs-field-uses-insecure-uri",
                            vec![format!("{} {}", field_name, url)],
                        );

                        if !issue.should_fix(base_path) {
                            overridden_issues.push(issue);
                        } else {
                            source.set_vcs_url(vcs_type, &new_url);
                            fields_changed.insert(field_name);
                            fixed_issues.push(issue);
                        }
                    }
                }
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    // Build description
    let mut description_lines = Vec::new();
    if fields_changed.len() == 1 {
        let field = fields_changed.iter().next().unwrap();
        description_lines.push(format!("Use secure URI in Vcs control header {}.", field));
    } else {
        let fields_list = fields_changed
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        description_lines.push(format!(
            "Use secure URI in Vcs control headers: {}.",
            fields_list
        ));
    }

    if lp_note {
        description_lines.push(String::new());
        description_lines.push(
            "The lp: prefix gets expanded to http://code.launchpad.net/ for users that are \
             not logged in on some versions of Bazaar."
                .to_string(),
        );
    }

    let description = description_lines.join("\n");

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .build())
}

declare_fixer! {
    name: "vcs-field-uses-insecure-uri",
    tags: ["vcs-field-uses-insecure-uri"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_http_to_https_no_net_access() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\n\
             Vcs-Git: http://github.com/jelmer/test\n\n\
             Package: test-package\n\
             Description: Test\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, &preferences).unwrap();
        assert!(result
            .description
            .contains("Use secure URI in Vcs control header"));

        let content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(content.contains("Vcs-Git: https://github.com/jelmer/test"));
        assert!(!content.contains("http://github.com"));
    }

    #[test]
    fn test_already_https() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\n\
             Vcs-Git: https://github.com/jelmer/test\n\n\
             Package: test-package\n\
             Description: Test\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_vcs_fields() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\n\
             Vcs-Git: http://github.com/jelmer/test\n\
             Vcs-Browser: http://github.com/jelmer/test\n\n\
             Package: test-package\n\
             Description: Test\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, &preferences).unwrap();
        assert!(result
            .description
            .contains("Use secure URI in Vcs control headers"));

        let content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(content.contains("Vcs-Git: https://github.com/jelmer/test"));
        assert!(content.contains("Vcs-Browser: https://github.com/jelmer/test"));
    }

    #[test]
    fn test_no_vcs_fields() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\n\n\
             Package: test-package\n\
             Description: Test\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
