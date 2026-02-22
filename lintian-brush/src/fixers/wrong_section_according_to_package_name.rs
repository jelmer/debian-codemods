use crate::{declare_fixer, Certainty, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use lazy_regex::Regex;
use std::fs;
use std::path::Path;

const CERTAINTY: Certainty = Certainty::Likely;

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
    // Check minimum certainty
    if !crate::certainty_sufficient(CERTAINTY, preferences.minimum_certainty) {
        return Err(FixerError::NotCertainEnough(
            CERTAINTY,
            preferences.minimum_certainty,
            vec![],
        ));
    }

    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Load the name-to-section mappings
    let regexes = match get_name_section_mappings(preferences.lintian_data_path.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Failed to load name-section mappings: {}", e);
            return Err(FixerError::NoChanges);
        }
    };

    let editor = TemplatedControlEditor::open(&control_path)?;

    // Get default section from source paragraph
    let default_section = if let Some(source) = editor.source() {
        source.as_deb822().get("Section")
    } else {
        None
    };

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut changes = Vec::new();

    // Process binary packages
    for mut binary in editor.binaries() {
        let paragraph = binary.as_mut_deb822();

        let package_name = match paragraph.get("Package") {
            Some(name) => name.to_string(),
            None => continue,
        };

        let expected_section = match find_expected_section(&regexes, &package_name) {
            Some(s) => s,
            None => continue,
        };

        // Get current section (from binary or fall back to source default)
        let current_section = paragraph
            .get("Section")
            .or(default_section.clone())
            .unwrap_or_default();

        if expected_section != current_section {
            let issue = LintianIssue::binary_with_info(
                &package_name,
                "wrong-section-according-to-package-name",
                vec![format!("{} => {}", current_section, expected_section)],
            );

            if issue.should_fix(base_path) {
                changes.push((
                    package_name.clone(),
                    current_section.to_string(),
                    expected_section.to_string(),
                ));
                fixed_issues.push(issue);
                binary.set_section(Some(expected_section));
            } else {
                overridden_issues.push(issue);
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
    let changes_desc: Vec<String> = changes
        .iter()
        .map(|(pkg, old, new)| format!("binary package {} ({} ⇒ {})", pkg, old, new))
        .collect();

    let description = format!("Fix sections for {}.", changes_desc.join(", "));

    Ok(FixerResult::builder(&description)
        .certainty(CERTAINTY)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "wrong-section-according-to-package-name",
    tags: ["wrong-section-according-to-package-name"],
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
    fn test_fix_python_package_section() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Section: libs
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
        assert!(result.description.contains("python"));

        // Check that the section was updated
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Section: python"));
    }

    #[test]
    fn test_no_change_correct_section() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Section: python
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

        // Should not change if section is already correct
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_dbg_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Section: libs
Maintainer: Test User <test@example.com>

Package: test-pkg-dbg
Architecture: all
Description: Debug symbols
 Debug symbols
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);

        assert!(result.is_ok());

        // Check that the section was updated to debug
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Section: debug"));
    }

    #[test]
    fn test_minimum_certainty_not_met() {
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

        let preferences = FixerPreferences {
            minimum_certainty: Some(Certainty::Certain),
            ..Default::default()
        };
        let result = run(base_path, &preferences);

        // Should fail because CERTAINTY (Likely) < Certain
        assert!(matches!(result, Err(FixerError::NotCertainEnough(_, _, _))));
    }
}
