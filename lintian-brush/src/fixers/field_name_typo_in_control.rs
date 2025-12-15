use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::collections::HashSet;
use std::path::Path;

// Include the generated field definitions
include!(concat!(env!("OUT_DIR"), "/debian_control_fields.rs"));

/// Get the current vendor (e.g., "debian", "ubuntu")
fn get_vendor() -> String {
    // Try to get vendor from dpkg-vendor
    std::process::Command::new("dpkg-vendor")
        .arg("--query")
        .arg("vendor")
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "debian".to_string())
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut case_fixed = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Get the vendor
    let vendor = get_vendor();

    // Combine all valid field names for this vendor
    let mut valid_field_names = HashSet::new();
    valid_field_names.extend(known_debian_source_fields(&vendor));
    valid_field_names.extend(known_debian_binary_fields(&vendor));

    // Check source paragraph
    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();
        let keys: Vec<String> = paragraph.keys().map(|k| k.to_string()).collect();

        for field in keys {
            if valid_field_names.contains(field.as_str()) {
                continue;
            }

            // Look for a case-insensitive match
            for &valid_field in &valid_field_names {
                if valid_field.to_lowercase() == field.to_lowercase() {
                    // Found a case mismatch
                    let issue = LintianIssue::source_with_info(
                        "cute-field",
                        vec![format!("{} vs {}", field, valid_field)],
                    );

                    if !issue.should_fix(base_path) {
                        overridden_issues.push(issue);
                        break;
                    }

                    if let Some(value) = paragraph.get(&field) {
                        let value = value.to_string();
                        paragraph.remove(&field);
                        paragraph.set(valid_field, &value);
                        case_fixed.push((field.clone(), valid_field.to_string()));
                        fixed_issues.push(issue);
                        break;
                    }
                }
            }
        }
    }

    // Check binary paragraphs
    for mut binary in editor.binaries() {
        let Some(package_name) = binary.name() else {
            continue;
        };
        let paragraph = binary.as_mut_deb822();
        let keys: Vec<String> = paragraph.keys().map(|k| k.to_string()).collect();

        for field in keys {
            if valid_field_names.contains(field.as_str()) {
                continue;
            }

            // Look for a case-insensitive match
            for &valid_field in &valid_field_names {
                if valid_field.to_lowercase() == field.to_lowercase() {
                    // Found a case mismatch
                    let issue = LintianIssue::binary_with_info(
                        &package_name,
                        "cute-field",
                        vec![format!("{} vs {}", field, valid_field)],
                    );

                    if !issue.should_fix(base_path) {
                        overridden_issues.push(issue);
                        break;
                    }

                    if let Some(value) = paragraph.get(&field) {
                        let value = value.to_string();
                        paragraph.remove(&field);
                        paragraph.set(valid_field, &value);
                        case_fixed.push((field.clone(), valid_field.to_string()));
                        fixed_issues.push(issue);
                        break;
                    }
                }
            }
        }
    }

    if case_fixed.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    // Create the result message
    let kind = if case_fixed.len() > 1 {
        "cases"
    } else {
        "case"
    };
    // Sort the case_fixed list to ensure consistent ordering
    case_fixed.sort();
    let fixed_str = case_fixed
        .iter()
        .map(|(old, new)| format!("{} ⇒ {}", old, new))
        .collect::<Vec<_>>()
        .join(", ");

    let message = format!("Fix field name {} in debian/control ({}).", kind, fixed_str);

    Ok(FixerResult::builder(message)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "field-name-typo-in-control",
    tags: ["cute-field"],
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
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_fix_homepage_case() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nHomePage: https://www.example.com/\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Fix field name case in debian/control (HomePage ⇒ Homepage)."
        );

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Homepage: https://www.example.com/"));
        assert!(!updated_content.contains("HomePage:"));
    }

    #[test]
    fn test_no_typos() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nHomepage: https://www.example.com/\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_typos() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nHomePage: https://www.example.com/\nmaintainer: John Doe <john@example.com>\n\nPackage: test\narchitecture: any\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.description, "Fix field name cases in debian/control (HomePage ⇒ Homepage, architecture ⇒ Architecture, maintainer ⇒ Maintainer).");

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Homepage:"));
        assert!(updated_content.contains("Maintainer:"));
        assert!(updated_content.contains("Architecture:"));
    }
}
