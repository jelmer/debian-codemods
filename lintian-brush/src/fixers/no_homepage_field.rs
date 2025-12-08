use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;
use url::Url;

fn convert_certainty(upstream_certainty: upstream_ontologist::Certainty) -> crate::Certainty {
    match upstream_certainty {
        upstream_ontologist::Certainty::Certain => crate::Certainty::Certain,
        upstream_ontologist::Certainty::Confident => crate::Certainty::Confident,
        upstream_ontologist::Certainty::Likely => crate::Certainty::Likely,
        upstream_ontologist::Certainty::Possible => crate::Certainty::Possible,
    }
}

fn guess_homepage(
    base_path: &Path,
    preferences: &FixerPreferences,
) -> Option<(String, upstream_ontologist::Certainty)> {
    // Create a tokio runtime to call the async function
    let rt = tokio::runtime::Runtime::new().ok()?;

    let trust_package = if preferences.trust_package.unwrap_or(false) {
        Some(true)
    } else {
        None
    };

    let net_access = preferences.net_access;

    rt.block_on(async {
        // Use guess_upstream_metadata which accepts net_access parameter
        let metadata = upstream_ontologist::guess_upstream_metadata(
            base_path,
            trust_package,
            net_access,
            None, // consult_external_directory
            None, // check
        )
        .await
        .ok()?;

        // Get the Homepage datum with metadata
        if let Some(homepage_datum) = metadata.get("Homepage") {
            // Skip homepages from debian/control (known bad guess)
            // TODO: This logic should be shared with upstream-metadata-file fixer,
            // which uses filter_bad_guesses(). Consider adding a Rust equivalent
            // of filter_bad_guesses() to upstream-ontologist or debian-analyzer.
            if let Some(ref origin) = homepage_datum.origin {
                let origin_str = origin.to_string();
                if origin_str == "./debian/control" || origin_str == "debian/control" {
                    return None;
                }
            }

            // Extract the URL from the datum
            if let upstream_ontologist::UpstreamDatum::Homepage(url) = &homepage_datum.datum {
                let certainty = homepage_datum
                    .certainty
                    .unwrap_or(upstream_ontologist::Certainty::Possible);
                return Some((url.clone(), certainty));
            }
        }

        None
    })
}

pub fn run(
    base_path: &Path,
    _package_name: &str,
    preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut made_changes = false;
    let mut description = String::new();
    let mut certainty = crate::Certainty::Certain;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    if let Some(mut source) = editor.source() {
        let source_para = source.as_mut_deb822();

        if !source_para.contains_key("Homepage") {
            // No Homepage field exists
            if let Some((homepage_url, upstream_certainty)) = guess_homepage(base_path, preferences)
            {
                let issue = LintianIssue::source_with_info("no-homepage-field", vec![String::new()]);

                if issue.should_fix(base_path) {
                    source_para.set("Homepage", &homepage_url);
                    made_changes = true;
                    description = "Fill in Homepage field".to_string();
                    certainty = convert_certainty(upstream_certainty);
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
            }
        } else {
            // Homepage field exists, check if it's a pypi.org or rubygems.org URL
            let homepage = source_para.get("Homepage").ok_or(FixerError::NoChanges)?;

            if let Ok(url) = Url::parse(&homepage) {
                let hostname = url.host_str();
                let (tag, should_replace) = match hostname {
                    Some("pypi.org") => (Some("pypi-homepage"), true),
                    Some("rubygems.org") => (Some("rubygem-homepage"), true),
                    _ => (None, false),
                };

                if should_replace {
                    if let Some((homepage_url, upstream_certainty)) =
                        guess_homepage(base_path, preferences)
                    {
                        let issue = LintianIssue::source_with_info(tag.unwrap(), vec![homepage.clone()]);

                        if issue.should_fix(base_path) {
                            source_para.set("Homepage", &homepage_url);
                            made_changes = true;
                            description = format!("Avoid {} in Homepage field", hostname.unwrap());
                            certainty = convert_certainty(upstream_certainty);
                            fixed_issues.push(issue);
                        } else {
                            overridden_issues.push(issue);
                        }
                    }
                }
            }
        }
    }

    if !made_changes {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    Ok(FixerResult::builder(description)
        .certainty(certainty)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "no-homepage-field",
    tags: ["no-homepage-field", "pypi-homepage", "rubygem-homepage"],
    apply: |basedir, package, _version, preferences| {
        run(basedir, package, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_no_homepage_field_exists() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Maintainer: Test User <test@example.com>

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, "test-package", &preferences);
        // Should not make changes without network access or trust_package
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_homepage_already_exists() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Homepage: https://example.com
Maintainer: Test User <test@example.com>

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, "test-package", &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_pypi_homepage() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Homepage: https://pypi.org/project/test-package/
Maintainer: Test User <test@example.com>

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, "test-package", &preferences);
        // Should not make changes without network access or trust_package
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_rubygems_homepage() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Homepage: https://rubygems.org/gems/test-package
Maintainer: Test User <test@example.com>

Package: test-package
Description: Test package
 This is a test package.
"#;
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, "test-package", &preferences);
        // Should not make changes without network access or trust_package
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
