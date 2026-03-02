use crate::{Certainty, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use lazy_regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Get the name-to-section mappings from lintian data
fn get_name_section_mappings(
    lintian_data_path: Option<&Path>,
) -> Result<Vec<(Regex, String)>, std::io::Error> {
    let mappings_path = if let Some(path) = lintian_data_path {
        path.join("fields/name_section_mappings")
    } else {
        Path::new("/usr/share/lintian/data/fields/name_section_mappings").to_path_buf()
    };

    let content = fs::read_to_string(&mappings_path)?;
    let mut regexes = Vec::new();

    for (lineno, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        if let Some((regex_str, section)) = line.split_once("=>") {
            let regex_str = regex_str.trim();
            let section = section.trim();

            match regex::Regex::new(regex_str) {
                Ok(regex) => {
                    regexes.push((regex, section.to_string()));
                }
                Err(e) => {
                    tracing::warn!(
                        "{}:{}: Invalid regex '{}': {}",
                        mappings_path.display(),
                        lineno + 1,
                        regex_str,
                        e
                    );
                    continue;
                }
            }
        }
    }

    Ok(regexes)
}

/// Find the expected section for a package name based on mappings
fn find_expected_section<'a>(regexes: &'a [(Regex, String)], name: &str) -> Option<&'a str> {
    for (regex, section) in regexes {
        if regex.is_match(name) {
            return Some(section);
        }
    }
    None
}

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    // Check if source already has a Section field
    if let Some(source) = editor.source() {
        if source.section().is_some() {
            return Err(FixerError::NoChanges);
        }
    }

    // Load the name-to-section mappings
    let regexes = match get_name_section_mappings(preferences.lintian_data_path.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Failed to load name-section mappings: {}", e);
            return Err(FixerError::NoChanges);
        }
    };

    let mut binary_sections_set = HashSet::new();
    let mut binary_sections = HashSet::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // First pass: set sections on binaries that don't have them
    for mut binary in editor.binaries() {
        if binary.section().is_none() {
            let package_name = match binary.name() {
                Some(name) => name.to_string(),
                None => continue,
            };

            if let Some(section) = find_expected_section(&regexes, &package_name) {
                let line_no = binary.as_deb822().line() + 1; // Convert to 1-indexed
                let issue = LintianIssue {
                    package: Some(package_name.clone()),
                    package_type: Some(crate::PackageType::Binary),
                    tag: Some("recommended-field".to_string()),
                    info: Some(format!(
                        "(in section for {}) Section [debian/control:{}]",
                        package_name, line_no
                    )),
                };

                if issue.should_fix(base_path) {
                    binary.set_section(Some(section));
                    binary_sections_set.insert(package_name);
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
            }
        }

        // Collect all sections from binaries
        if let Some(section) = binary.section() {
            binary_sections.insert(section.to_string());
        }
    }

    let mut source_section_set = false;

    // If all binaries have the same section, move it to source
    if binary_sections.len() == 1 {
        let section = binary_sections.iter().next().unwrap().clone();

        if let Some(mut source) = editor.source() {
            let source_line = source.as_deb822().line() + 1;
            let issue = LintianIssue {
                package: None,
                package_type: Some(crate::PackageType::Source),
                tag: Some("recommended-field".to_string()),
                info: Some(format!(
                    "(in section for source) Section [debian/control:{}]",
                    source_line
                )),
            };

            if issue.should_fix(base_path) {
                // Set section on source
                source.set_section(Some(&section));
                source_section_set = true;
                fixed_issues.push(issue);

                // Remove section from binaries that have the same section
                for mut binary in editor.binaries() {
                    if let Some(bin_section) = binary.section() {
                        if bin_section == section {
                            binary.set_section(None);
                        }
                    }
                }
            } else {
                overridden_issues.push(issue);
            }
        }
    }

    if !source_section_set && binary_sections_set.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    // Build description based on what was done
    let description = if source_section_set && !binary_sections_set.is_empty() {
        "Section field set in source based on binary package names.".to_string()
    } else if source_section_set {
        "Section field set in source stanza rather than binary packages.".to_string()
    } else {
        let mut packages: Vec<_> = binary_sections_set.iter().cloned().collect();
        packages.sort();
        format!(
            "Section field set for binary packages {} based on name.",
            packages.join(", ")
        )
    };

    Ok(FixerResult::builder(&description)
        .certainty(Certainty::Certain)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "no-section-field",
    tags: ["recommended-field"],
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
    fn test_no_control() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_source_already_has_section() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Section: libs
Maintainer: Test User <test@example.com>

Package: test-pkg
Architecture: all
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);

        // Should not change if source already has section
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_set_section_on_binary() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Maintainer: Test User <test@example.com>

Package: python3-testpkg
Architecture: all
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);

        assert!(result.is_ok());
        let result = result.unwrap();
        // When there's only one binary, the section gets set on binary first,
        // then moved to source
        assert_eq!(
            result.description,
            "Section field set in source based on binary package names."
        );

        // Check that section was moved to source
        let editor = TemplatedControlEditor::open(&control_path).unwrap();
        let source = editor.source().unwrap();
        assert_eq!(source.as_deb822().get("Section").as_deref(), Some("python"));

        // Binary should not have Section
        let binary = editor.binaries().next().unwrap();
        assert_eq!(binary.as_deb822().get("Section"), None);
    }

    #[test]
    fn test_move_section_to_source() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Maintainer: Test User <test@example.com>

Package: python3-testpkg
Architecture: all
Section: python
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Section field set in source stanza rather than binary packages."
        );

        // Check that section was moved to source
        let editor = TemplatedControlEditor::open(&control_path).unwrap();

        // Source should have Section
        let source = editor.source().unwrap();
        assert_eq!(source.as_deb822().get("Section").as_deref(), Some("python"));

        // Binary should not have Section
        let binary = editor.binaries().next().unwrap();
        assert_eq!(binary.as_deb822().get("Section"), None);
    }

    #[test]
    fn test_multiple_binaries_same_section() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Maintainer: Test User <test@example.com>

Package: python3-testpkg
Architecture: all
Description: Test package
 Test description

Package: python3-testpkg-extra
Architecture: all
Description: Extra package
 Extra description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Section field set in source based on binary package names."
        );

        // Both packages should get python section, then moved to source
        let editor = TemplatedControlEditor::open(&control_path).unwrap();

        // Source should have Section
        let source = editor.source().unwrap();
        assert_eq!(source.as_deb822().get("Section").as_deref(), Some("python"));

        // Binaries should not have Section
        for binary in editor.binaries() {
            assert_eq!(binary.as_deb822().get("Section"), None);
        }
    }

    #[test]
    fn test_multiple_binaries_different_sections() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Maintainer: Test User <test@example.com>

Package: python3-testpkg
Architecture: all
Description: Test package
 Test description

Package: test-pkg-doc
Architecture: all
Description: Documentation
 Documentation
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Section field set for binary packages python3-testpkg, test-pkg-doc based on name."
        );

        // Packages should have different sections, so they stay on binaries
        let editor = TemplatedControlEditor::open(&control_path).unwrap();

        // Source should not have Section
        let source = editor.source().unwrap();
        assert_eq!(source.as_deb822().get("Section"), None);

        // Check binary sections
        let mut binaries: Vec<_> = editor.binaries().collect();
        binaries.sort_by_key(|b| b.as_deb822().get("Package").unwrap_or_default());

        assert_eq!(
            binaries[0].as_deb822().get("Package").as_deref(),
            Some("python3-testpkg")
        );
        assert_eq!(
            binaries[0].as_deb822().get("Section").as_deref(),
            Some("python")
        );

        assert_eq!(
            binaries[1].as_deb822().get("Package").as_deref(),
            Some("test-pkg-doc")
        );
        assert_eq!(
            binaries[1].as_deb822().get("Section").as_deref(),
            Some("doc")
        );
    }
}
