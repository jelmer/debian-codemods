use crate::{declare_fixer, Certainty, FixerError, FixerPreferences, FixerResult, LintianIssue};
use deb822_lossless::Deb822;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Read the list of known obsolete restrictions from lintian data
fn read_obsolete_restrictions(
    lintian_data_path: Option<&Path>,
) -> Result<HashSet<String>, FixerError> {
    let default_path = PathBuf::from("/usr/share/lintian/data");
    let lintian_data_path = lintian_data_path.unwrap_or(&default_path);

    let path = lintian_data_path
        .join("testsuite")
        .join("known-obsolete-restrictions");

    if !path.exists() {
        return Err(FixerError::Other("Lintian data file not found".to_string()));
    }

    let content = fs::read_to_string(&path)?;
    let mut restrictions = HashSet::new();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        restrictions.insert(line.to_string());
    }

    Ok(restrictions)
}

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/tests/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let deprecated_restrictions =
        read_obsolete_restrictions(preferences.lintian_data_path.as_deref())?;
    let mut removed_restrictions = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut overall_certainty = Certainty::Certain;

    // Parse the tests control file using lossless parser
    let content = fs::read_to_string(&control_path)?;
    let parsed = Deb822::parse(&content);
    let deb822 = parsed.tree();

    for mut paragraph in deb822.paragraphs() {
        let restrictions_entry = match paragraph
            .entries()
            .find(|e| e.key().as_deref() == Some("Restrictions"))
        {
            Some(entry) => entry,
            None => continue,
        };

        let restrictions_value = restrictions_entry.value();
        let line_num = restrictions_entry.line() + 1;

        let restrictions: Vec<&str> = restrictions_value
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        if restrictions.is_empty() {
            continue;
        }

        let mut to_delete = Vec::new();
        for restriction in &restrictions {
            if deprecated_restrictions.contains(*restriction) {
                let certainty = if *restriction == "needs-recommends" {
                    Certainty::Possible
                } else {
                    Certainty::Certain
                };

                let issue = LintianIssue::source_with_info(
                    "obsolete-runtime-tests-restriction",
                    vec![format!(
                        "{} [debian/tests/control:{}]",
                        restriction, line_num
                    )],
                );

                if issue.should_fix(base_path) {
                    to_delete.push(restriction.to_string());
                    overall_certainty = crate::min_certainty(&[overall_certainty, certainty])
                        .unwrap_or(overall_certainty);
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
            }
        }

        if !to_delete.is_empty() {
            removed_restrictions.extend(to_delete.iter().cloned());

            // Remove obsolete restrictions
            let new_restrictions: Vec<String> = restrictions
                .iter()
                .filter(|r| !to_delete.contains(&r.to_string()))
                .map(|s| s.to_string())
                .collect();

            if new_restrictions.is_empty() {
                paragraph.remove("Restrictions");
            } else {
                paragraph.set("Restrictions", &new_restrictions.join(", "));
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Write back the modified file
    fs::write(&control_path, deb822.to_string())?;

    let plural = if removed_restrictions.len() > 1 {
        "s"
    } else {
        ""
    };
    let description = format!(
        "Drop deprecated restriction{} {}. See https://salsa.debian.org/ci-team/autopkgtest/tree/master/doc/README.package-tests.rst",
        plural,
        removed_restrictions.join(", ")
    );

    Ok(FixerResult::builder(description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .certainty(overall_certainty)
        .build())
}

declare_fixer! {
    name: "obsolete-runtime-tests-restriction",
    tags: ["obsolete-runtime-tests-restriction"],
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
    fn test_remove_obsolete_restriction() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        // Create a control file with an obsolete restriction
        let control_content = r#"Tests: test1
Restrictions: needs-root, rw-build-tree
Depends: @

Tests: test2
Restrictions: breaks-testbed
"#;
        let control_path = tests_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        // Create a mock known-obsolete-restrictions file
        let lintian_data_dir = temp_dir.path().join("lintian-data/testsuite");
        fs::create_dir_all(&lintian_data_dir).unwrap();
        let obsolete_file = lintian_data_dir.join("known-obsolete-restrictions");
        fs::write(&obsolete_file, "rw-build-tree\n").unwrap();

        let preferences = FixerPreferences {
            lintian_data_path: Some(temp_dir.path().join("lintian-data")),
            ..Default::default()
        };

        let result = run(temp_dir.path(), &preferences);
        assert!(result.is_ok(), "Error: {:?}", result);

        let result = result.unwrap();
        assert!(result.description.contains("rw-build-tree"));
        assert_eq!(result.certainty, Some(Certainty::Certain));

        // Verify the file was updated
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Restrictions: needs-root"));
        assert!(!updated_content.contains("rw-build-tree"));
    }

    #[test]
    fn test_remove_all_restrictions() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let control_content = r#"Tests: test1
Restrictions: rw-build-tree
Depends: @
"#;
        let control_path = tests_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let lintian_data_dir = temp_dir.path().join("lintian-data/testsuite");
        fs::create_dir_all(&lintian_data_dir).unwrap();
        let obsolete_file = lintian_data_dir.join("known-obsolete-restrictions");
        fs::write(&obsolete_file, "rw-build-tree\n").unwrap();

        let preferences = FixerPreferences {
            lintian_data_path: Some(temp_dir.path().join("lintian-data")),
            ..Default::default()
        };

        let result = run(temp_dir.path(), &preferences);
        assert!(result.is_ok());

        // Verify Restrictions field was removed entirely
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("Restrictions:"));
    }

    #[test]
    fn test_needs_recommends_certainty() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let control_content = r#"Tests: test1
Restrictions: needs-recommends
"#;
        let control_path = tests_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let lintian_data_dir = temp_dir.path().join("lintian-data/testsuite");
        fs::create_dir_all(&lintian_data_dir).unwrap();
        let obsolete_file = lintian_data_dir.join("known-obsolete-restrictions");
        fs::write(&obsolete_file, "needs-recommends\n").unwrap();

        let preferences = FixerPreferences {
            lintian_data_path: Some(temp_dir.path().join("lintian-data")),
            ..Default::default()
        };

        let result = run(temp_dir.path(), &preferences);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.certainty, Some(Certainty::Possible));
    }

    #[test]
    fn test_no_changes_when_no_obsolete() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        let control_content = r#"Tests: test1
Restrictions: needs-root
"#;
        let control_path = tests_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let lintian_data_dir = temp_dir.path().join("lintian-data/testsuite");
        fs::create_dir_all(&lintian_data_dir).unwrap();
        let obsolete_file = lintian_data_dir.join("known-obsolete-restrictions");
        fs::write(&obsolete_file, "rw-build-tree\n").unwrap();

        let preferences = FixerPreferences {
            lintian_data_path: Some(temp_dir.path().join("lintian-data")),
            ..Default::default()
        };

        let result = run(temp_dir.path(), &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
