use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_control::lossless::relations::Relations;
use std::path::Path;

/// Check if an entry is implied by any entry in a list of stronger relations
fn is_implied_by_any(
    entry: &debian_control::lossless::relations::Entry,
    stronger_relations: &[&Relations],
) -> bool {
    for relations in stronger_relations {
        for stronger_entry in relations.entries() {
            if entry.is_implied_by(&stronger_entry) {
                return true;
            }
        }
    }
    false
}

/// Remove redundant entries from a weaker dependency field
fn remove_redundant_entries(
    package_name: &str,
    field_name: &str,
    field_value: &str,
    stronger_relations: &[&Relations],
    base_path: &Path,
    fixed_issues: &mut Vec<LintianIssue>,
    overridden_issues: &mut Vec<LintianIssue>,
) -> Option<String> {
    if field_value.is_empty() {
        return None;
    }

    let (mut relations, _) = Relations::parse_relaxed(field_value, true);
    let mut entries_to_remove = Vec::new();

    for (idx, entry) in relations.entries().enumerate() {
        if is_implied_by_any(&entry, stronger_relations) {
            let package_names: Vec<String> =
                entry.relations().filter_map(|r| r.try_name()).collect();

            let mut should_remove = true;
            for pkg in &package_names {
                let issue = LintianIssue::binary_with_info(
                    package_name,
                    "redundant-installation-prerequisite",
                    vec![format!("{} in {}", pkg, field_name)],
                );

                if !issue.should_fix(base_path) {
                    overridden_issues.push(issue);
                    should_remove = false;
                    break;
                }
                fixed_issues.push(issue);
            }

            if should_remove {
                entries_to_remove.push(idx);
            }
        }
    }

    if entries_to_remove.is_empty() {
        return None;
    }

    for idx in entries_to_remove.iter().rev() {
        relations.remove_entry(*idx);
    }

    let result = relations.to_string();
    if result.trim().is_empty() {
        Some(String::new())
    } else {
        Some(result)
    }
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for mut binary in editor.binaries() {
        let package_name = match binary.name() {
            Some(name) => name.to_string(),
            None => continue,
        };

        let paragraph = binary.as_mut_deb822();

        let depends = paragraph.get("Depends").unwrap_or_default();
        let pre_depends = paragraph.get("Pre-Depends").unwrap_or_default();
        let recommends = paragraph.get("Recommends").unwrap_or_default();
        let suggests = paragraph.get("Suggests").unwrap_or_default();

        let (depends_rel, _) = Relations::parse_relaxed(&depends, true);
        let (pre_depends_rel, _) = Relations::parse_relaxed(&pre_depends, true);
        let (recommends_rel, _) = Relations::parse_relaxed(&recommends, true);

        // Fix Recommends: remove if implied by Depends or Pre-Depends
        if let Some(new_value) = remove_redundant_entries(
            &package_name,
            "Recommends",
            &recommends,
            &[&depends_rel, &pre_depends_rel],
            base_path,
            &mut fixed_issues,
            &mut overridden_issues,
        ) {
            if new_value.is_empty() {
                paragraph.remove("Recommends");
            } else {
                paragraph.set("Recommends", &new_value);
            }
        }

        // Fix Suggests: remove if implied by Depends, Pre-Depends, or Recommends
        if let Some(new_value) = remove_redundant_entries(
            &package_name,
            "Suggests",
            &suggests,
            &[&depends_rel, &pre_depends_rel, &recommends_rel],
            base_path,
            &mut fixed_issues,
            &mut overridden_issues,
        ) {
            if new_value.is_empty() {
                paragraph.remove("Suggests");
            } else {
                paragraph.set("Suggests", &new_value);
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

    let description = if fixed_issues.len() == 1 {
        "Remove redundant dependency from weaker field.".to_string()
    } else {
        format!(
            "Remove {} redundant dependencies from weaker fields.",
            fixed_issues.len()
        )
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "redundant-installation-prerequisite",
    tags: ["redundant-installation-prerequisite"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_remove_from_recommends() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: mypackage

Package: mypackage
Architecture: any
Depends: foo
Recommends: foo, bar
Description: Test package
 Test
"#;

        fs::write(debian_dir.join("control"), control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(base_path, "mypackage", &version, &Default::default());
        assert!(result.is_ok());

        let updated_control = fs::read_to_string(debian_dir.join("control")).unwrap();
        let expected_control = r#"Source: mypackage

Package: mypackage
Architecture: any
Depends: foo
Recommends: bar
Description: Test package
 Test
"#;
        assert_eq!(updated_control, expected_control);
    }

    #[test]
    fn test_remove_from_suggests() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: mypackage

Package: mypackage
Architecture: any
Depends: foo
Suggests: foo, baz
Description: Test package
 Test
"#;

        fs::write(debian_dir.join("control"), control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(base_path, "mypackage", &version, &Default::default());
        assert!(result.is_ok());

        let updated_control = fs::read_to_string(debian_dir.join("control")).unwrap();
        let expected_control = r#"Source: mypackage

Package: mypackage
Architecture: any
Depends: foo
Suggests: baz
Description: Test package
 Test
"#;
        assert_eq!(updated_control, expected_control);
    }

    #[test]
    fn test_remove_from_both() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: mypackage

Package: mypackage
Architecture: any
Depends: foo
Recommends: foo, bar
Suggests: foo, bar, baz
Description: Test package
 Test
"#;

        fs::write(debian_dir.join("control"), control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(base_path, "mypackage", &version, &Default::default());
        assert!(result.is_ok());

        let updated_control = fs::read_to_string(debian_dir.join("control")).unwrap();
        let expected_control = r#"Source: mypackage

Package: mypackage
Architecture: any
Depends: foo
Recommends: bar
Suggests: baz
Description: Test package
 Test
"#;
        assert_eq!(updated_control, expected_control);
    }

    #[test]
    fn test_remove_entire_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: mypackage

Package: mypackage
Architecture: any
Depends: foo
Recommends: foo
Description: Test package
 Test
"#;

        fs::write(debian_dir.join("control"), control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(base_path, "mypackage", &version, &Default::default());
        assert!(result.is_ok());

        let updated_control = fs::read_to_string(debian_dir.join("control")).unwrap();
        let expected_control = r#"Source: mypackage

Package: mypackage
Architecture: any
Depends: foo
Description: Test package
 Test
"#;
        assert_eq!(updated_control, expected_control);
    }

    #[test]
    fn test_no_redundancy() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: mypackage

Package: mypackage
Architecture: any
Depends: foo
Recommends: bar
Suggests: baz
Description: Test package
 Test
"#;

        fs::write(debian_dir.join("control"), control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(base_path, "mypackage", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_pre_depends_stronger_than_depends() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: mypackage

Package: mypackage
Architecture: any
Pre-Depends: foo
Recommends: foo, bar
Description: Test package
 Test
"#;

        fs::write(debian_dir.join("control"), control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(base_path, "mypackage", &version, &Default::default());
        assert!(result.is_ok());

        let updated_control = fs::read_to_string(debian_dir.join("control")).unwrap();
        let expected_control = r#"Source: mypackage

Package: mypackage
Architecture: any
Pre-Depends: foo
Recommends: bar
Description: Test package
 Test
"#;
        assert_eq!(updated_control, expected_control);
    }
}
