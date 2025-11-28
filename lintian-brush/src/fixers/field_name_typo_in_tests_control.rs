use crate::{declare_fixer, FixerError, FixerResult};
use deb822_lossless::Deb822;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::str::FromStr;

const VALID_FIELD_NAMES: &[&str] = &[
    "Tests",
    "Restrictions",
    "Features",
    "Depends",
    "Tests-Directory",
    "Test-Command",
];

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let tests_control_path = base_path.join("debian/tests/control");

    if !tests_control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&tests_control_path)?;
    let mut deb822 = Deb822::from_str(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/tests/control: {:?}", e)))?;

    let valid_fields: HashSet<&str> = VALID_FIELD_NAMES.iter().copied().collect();
    let mut case_fixed = Vec::new();
    let mut typo_fixed = Vec::new();

    for mut paragraph in deb822.paragraphs() {
        let field_names: Vec<String> = paragraph.keys().collect();

        for field_name in field_names {
            if valid_fields.contains(field_name.as_str()) {
                continue;
            }

            // Find a valid field with Levenshtein distance of 1
            for &valid_field in VALID_FIELD_NAMES {
                if strsim::levenshtein(&field_name, valid_field) == 1 {
                    let value = paragraph.get(&field_name).ok_or(FixerError::NoChanges)?;
                    let value_str = value.to_string();

                    paragraph.remove(&field_name);
                    paragraph.insert(valid_field, &value_str);

                    if valid_field.eq_ignore_ascii_case(&field_name) {
                        case_fixed.push((field_name.clone(), valid_field.to_string()));
                    } else {
                        typo_fixed.push((field_name.clone(), valid_field.to_string()));
                    }

                    break;
                }
            }
        }
    }

    if case_fixed.is_empty() && typo_fixed.is_empty() {
        return Err(FixerError::NoChanges);
    }

    fs::write(&tests_control_path, deb822.to_string())?;

    let kind = if !case_fixed.is_empty() && !typo_fixed.is_empty() {
        format!(
            "{} and {}",
            if case_fixed.len() > 1 {
                "cases"
            } else {
                "case"
            },
            if typo_fixed.len() > 1 {
                "typos"
            } else {
                "typo"
            }
        )
    } else if !case_fixed.is_empty() {
        if case_fixed.len() > 1 {
            "cases".to_string()
        } else {
            "case".to_string()
        }
    } else {
        if typo_fixed.len() > 1 {
            "typos".to_string()
        } else {
            "typo".to_string()
        }
    };

    let mut all_fixes = case_fixed;
    all_fixes.extend(typo_fixed);
    all_fixes.sort();

    let fixed_str = all_fixes
        .iter()
        .map(|(old, new)| format!("{} ⇒ {}", old, new))
        .collect::<Vec<_>>()
        .join(", ");

    Ok(FixerResult::builder(&format!(
        "Fix field name {} in debian/tests/control ({}).",
        kind, fixed_str
    ))
    .build())
}

declare_fixer! {
    name: "field-name-typo-in-tests-control",
    tags: ["field-name-typo-in-tests-control"],
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
    fn test_typo_fix() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        fs::write(
            tests_dir.join("control"),
            "Tests: 4.08.1 ocaml-system\nDepends: @, ca-certificates\nRestrictions: isolation-container, allow-stderr\n\nTest: ocaml-system\nDepends: ocaml-nox\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Fix field name typo in debian/tests/control (Test ⇒ Tests)."
        );

        let content = fs::read_to_string(tests_dir.join("control")).unwrap();
        assert!(content.contains("Tests: ocaml-system"));
        assert!(!content.contains("Test: ocaml-system"));
    }

    #[test]
    fn test_case_fix() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        fs::write(tests_dir.join("control"), "tests: some-test\nDepends: @\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Fix field name case in debian/tests/control (tests ⇒ Tests)."
        );

        let content = fs::read_to_string(tests_dir.join("control")).unwrap();
        assert!(content.contains("Tests: some-test"));
        assert!(!content.contains("tests: some-test"));
    }

    #[test]
    fn test_multiple_fixes() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        fs::write(
            tests_dir.join("control"),
            "tests: test1\nDepend: foo\n\nTest: test2\nrestrictions: needs-root\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        // Should fix: tests→Tests (case), Depend→Depends (typo), Test→Tests (typo), restrictions→Restrictions (case)
        assert!(result.description.contains("debian/tests/control"));

        let content = fs::read_to_string(tests_dir.join("control")).unwrap();
        assert!(content.contains("Tests: test1"));
        assert!(content.contains("Depends: foo"));
        assert!(content.contains("Tests: test2"));
        assert!(content.contains("Restrictions: needs-root"));
    }

    #[test]
    fn test_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_needed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        fs::write(
            tests_dir.join("control"),
            "Tests: some-test\nDepends: @\nRestrictions: needs-root\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_only_distance_one() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let tests_dir = debian_dir.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();

        // "Foo" has distance > 1 from all valid fields, should not be fixed
        fs::write(tests_dir.join("control"), "Foo: some-test\nDepends: @\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
