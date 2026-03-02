use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::abstract_control::AbstractControlEditor;
use debian_analyzer::control::TemplatedControlEditor;
use debian_analyzer::debcargo::DebcargoEditor;
use std::collections::BTreeSet;
use std::path::Path;

/// Helper function to get the appropriate control editor for a package
/// TODO: This should be provided by the fixer framework as part of a context object
fn get_control_editor(base_path: &Path) -> Result<Box<dyn AbstractControlEditor>, FixerError> {
    let control_path = base_path.join("debian/control");
    let debcargo_path = base_path.join("debian/debcargo.toml");

    if debcargo_path.exists() && !control_path.exists() {
        // Use DebcargoEditor for debcargo packages
        // DebcargoEditor::from_directory expects the base path, not the debian dir
        Ok(Box::new(DebcargoEditor::from_directory(base_path)?))
    } else {
        // Use TemplatedControlEditor for regular packages
        Ok(Box::new(TemplatedControlEditor::open(&control_path)?))
    }
}

/// Canonicalize a VCS URL based on its type
fn canonicalize_vcs_url(vcs_type: &str, url: &str) -> String {
    match vcs_type {
        "Browser" => debian_analyzer::vcs::canonicalize_vcs_browser_url(url),
        "Git" => {
            // Use upstream_ontologist::vcs::canonicalize_vcs_url if available
            // For now, replicate the Python logic: split, canonicalize repo, unsplit
            match url.parse::<debian_control::vcs::ParsedVcs>() {
                Ok(mut parsed) => {
                    // Use tokio runtime to call async canonical_git_repo_url function
                    let rt = tokio::runtime::Runtime::new().unwrap();

                    // Parse repo URL and canonicalize it
                    if let Ok(repo_url) = url::Url::parse(&parsed.repo_url) {
                        if let Some(canonical_url) = rt.block_on(
                            upstream_ontologist::vcs::canonical_git_repo_url(&repo_url, None),
                        ) {
                            parsed.repo_url = canonical_url.to_string();
                        }
                    }

                    parsed.to_string()
                }
                Err(_) => url.to_string(), // Return unchanged if parsing fails
            }
        }
        _ => url.to_string(), // Return unchanged for other VCS types
    }
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let mut editor = get_control_editor(base_path)?;
    let mut fields_changed = BTreeSet::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    if let Some(mut source) = editor.source() {
        // TODO: We shouldn't hardcode this list of VCS types.
        // Ideally, Source should provide a way to iterate over all VCS fields present.
        let vcs_types = vec![
            "Git", "Browser", "Svn", "Bzr", "Hg", "Cvs", "Arch", "Darcs", "Mtn", "Svk",
        ];

        for vcs_type in vcs_types {
            if let Some(url) = source.get_vcs_url(vcs_type) {
                let new_value = canonicalize_vcs_url(vcs_type, &url);

                if new_value != url {
                    let field_name = format!("Vcs-{}", vcs_type);
                    let issue = LintianIssue::source_with_info(
                        "vcs-field-not-canonical",
                        vec![format!("{} {} {}", vcs_type, url, new_value)],
                    );

                    if !issue.should_fix(base_path) {
                        overridden_issues.push(issue);
                    } else {
                        source.set_vcs_url(vcs_type, &new_value);
                        fields_changed.insert(field_name);
                        fixed_issues.push(issue);
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

    editor.commit();

    let fields_list = fields_changed.into_iter().collect::<Vec<_>>().join(", ");
    let description = format!("Use canonical URL in {}.", fields_list);

    Ok(FixerResult::builder(description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "vcs-field-not-canonical",
    tags: ["vcs-field-not-canonical"],
    // Must canonicalize URIs after fixing type mismatches and before securing them
    after: ["vcs-field-mismatch"],
    before: ["vcs-field-uses-insecure-uri"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_canonicalize_browser_url() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Vcs-Browser: https://bzr.debian.org/loggerhead/pkg-bazaar/bzr

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let result = run(base_path);
        assert!(result.is_ok(), "Error: {:?}", result);

        let result = result.unwrap();
        assert_eq!(result.description, "Use canonical URL in Vcs-Browser.");
        assert_eq!(result.certainty, None);

        // Check that the file was updated correctly
        let expected_content = r#"Source: test-package
Vcs-Browser: https://anonscm.debian.org/loggerhead/pkg-bazaar/bzr

Package: test-package
Description: Test package
 This is a test package.
"#;
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(updated_content, expected_content);
    }

    #[test]
    fn test_no_change_git_url() {
        // Test that git:// URLs that don't have canonical forms remain unchanged
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Vcs-Git: git://github.com/user/repo.git

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let result = run(base_path);
        // This should return NoChanges since canonical_git_repo_url doesn't modify this URL
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_canonical() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        // Use URLs that are already canonical (with .git suffix for Vcs-Git)
        let control_content = r#"Source: test-package
Vcs-Git: https://github.com/user/repo.git
Vcs-Browser: https://github.com/user/repo

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_vcs_fields() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Vcs-Git: git://salsa.debian.org/team/package
Vcs-Browser: https://bzr.debian.org/loggerhead/pkg-bazaar/bzr

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let result = run(base_path);
        assert!(result.is_ok(), "Error: {:?}", result);

        let result = result.unwrap();
        // Fields are sorted in BTreeSet, so they appear in alphabetical order
        assert_eq!(
            result.description,
            "Use canonical URL in Vcs-Browser, Vcs-Git."
        );
        assert_eq!(result.certainty, None);

        // Check that both fields were updated correctly
        // Note: canonical_git_repo_url adds .git suffix but doesn't change git:// to https://
        let expected_content = r#"Source: test-package
Vcs-Git: git://salsa.debian.org/team/package.git
Vcs-Browser: https://anonscm.debian.org/loggerhead/pkg-bazaar/bzr

Package: test-package
Description: Test package
 This is a test package.
"#;
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(updated_content, expected_content);
    }

    #[test]
    fn test_salsa_git_url() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Vcs-Git: https://salsa.debian.org/team/package

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let result = run(base_path);
        assert!(result.is_ok(), "Error: {:?}", result);

        let result = result.unwrap();
        assert_eq!(result.description, "Use canonical URL in Vcs-Git.");
        assert_eq!(result.certainty, None);

        // Check that .git was added
        let expected_content = r#"Source: test-package
Vcs-Git: https://salsa.debian.org/team/package.git

Package: test-package
Description: Test package
 This is a test package.
"#;
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(updated_content, expected_content);
    }
}
