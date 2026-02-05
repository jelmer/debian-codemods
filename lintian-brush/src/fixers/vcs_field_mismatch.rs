use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::collections::HashMap;
use std::path::Path;
use url::Url;

const HOST_TO_VCS: &[(&str, &str)] = &[
    ("github.com", "Git"),
    ("gitlab.com", "Git"),
    ("salsa.debian.org", "Git"),
];

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    let host_map: HashMap<&str, &str> = HOST_TO_VCS.iter().copied().collect();

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut old_vcs = String::new();
    let mut new_vcs = String::new();

    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        // Collect Vcs-* fields to check
        let vcs_fields: Vec<String> = paragraph
            .keys()
            .filter(|key| key.starts_with("Vcs-") && key.to_lowercase() != "vcs-browser")
            .map(|key| key.to_string())
            .collect();

        for field in vcs_fields {
            let vcs_type = &field[4..]; // Remove "Vcs-" prefix
            if let Some(vcs_url) = paragraph.get(&field) {
                // Parse the URL to get the hostname
                if let Ok(parsed_url) = Url::parse(&vcs_url) {
                    if let Some(host) = parsed_url.host_str() {
                        // Remove "user@" prefix if present
                        let clean_host = host.split('@').next_back().unwrap_or(host);

                        if let Some(&actual_vcs) = host_map.get(clean_host) {
                            if actual_vcs != vcs_type {
                                let vcs_url_value = vcs_url.to_string();

                                let issue = LintianIssue::source_with_info(
                                    "vcs-field-mismatch",
                                    vec![format!(
                                        "Vcs-{} != Vcs-{} {}",
                                        vcs_type, actual_vcs, vcs_url_value
                                    )],
                                );

                                if !issue.should_fix(base_path) {
                                    overridden_issues.push(issue);
                                } else {
                                    // Rename the field
                                    paragraph.rename(&field, &format!("Vcs-{}", actual_vcs));

                                    old_vcs = vcs_type.to_string();
                                    new_vcs = actual_vcs.to_string();
                                    fixed_issues.push(issue);
                                }
                            }
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

    Ok(FixerResult::builder(format!(
        "Changed vcs type from {} to {} based on URL.",
        old_vcs, new_vcs
    ))
    .fixed_issues(fixed_issues)
    .overridden_issues(overridden_issues)
    .build())
}

declare_fixer! {
    name: "vcs-field-mismatch",
    tags: ["vcs-field-mismatch"],
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
    fn test_simple() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: lintian-brush\nVcs-Bzr: https://salsa.debian.org/jelmer/dulwich.git\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Changed vcs type from Bzr to Git based on URL."
        );
        assert_eq!(result.certainty, None);

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(!content.contains("Vcs-Bzr"));
        assert!(content.contains("Vcs-Git: https://salsa.debian.org/jelmer/dulwich.git"));
    }

    #[test]
    fn test_no_op() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: lintian-brush\nVcs-Git: https://salsa.debian.org/jelmer/lintian-brush.git\nHomepage: https://www.jelmer.uk/lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
