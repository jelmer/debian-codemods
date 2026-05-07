use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences};
use deb822_lossless::Deb822;
use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;

const VALID_FIELD_NAMES: &[&str] = &[
    "Tests",
    "Restrictions",
    "Features",
    "Depends",
    "Tests-Directory",
    "Test-Command",
];

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rel = PathBuf::from("debian/tests/control");
    let bytes = match ws.read_file(&rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(Vec::new());
    };
    let deb822 = Deb822::from_str(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/tests/control: {:?}", e)))?;

    let valid_fields: HashSet<&str> = VALID_FIELD_NAMES.iter().copied().collect();
    let mut diagnostics = Vec::new();

    for (index, paragraph) in deb822.paragraphs().enumerate() {
        let field_names: Vec<String> = paragraph.keys().collect();
        for field_name in field_names {
            if valid_fields.contains(field_name.as_str()) {
                continue;
            }
            let Some(target) = VALID_FIELD_NAMES
                .iter()
                .copied()
                .find(|v| strsim::levenshtein(&field_name, v) == 1)
            else {
                continue;
            };
            let is_case = target.eq_ignore_ascii_case(&field_name);

            let message = format!(
                "{}\t{} ⇒ {}",
                if is_case { "case" } else { "typo" },
                field_name,
                target,
            );
            let actions = vec![Action::Deb822(Deb822Action::RenameField {
                file: rel.clone(),
                paragraph: ParagraphSelector::Index { index },
                from: field_name,
                to: target.to_string(),
            })];

            // Untagged: this fixer's lintian tag isn't tracked per-issue
            // by the original either (its FixerResult.fixed_lintian_issues
            // was empty).
            diagnostics.push(crate::diagnostic::Diagnostic::untagged(message, actions));
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let mut case_pairs: Vec<(String, String)> = Vec::new();
    let mut typo_pairs: Vec<(String, String)> = Vec::new();
    for diag in fixed {
        let Some((kind, rest)) = diag.message.split_once('\t') else {
            continue;
        };
        let Some((old, new)) = rest.split_once(" ⇒ ") else {
            continue;
        };
        let pair = (old.to_string(), new.to_string());
        if kind == "case" {
            case_pairs.push(pair);
        } else {
            typo_pairs.push(pair);
        }
    }

    let kind_str = match (!case_pairs.is_empty(), !typo_pairs.is_empty()) {
        (true, true) => format!(
            "{} and {}",
            if case_pairs.len() > 1 {
                "cases"
            } else {
                "case"
            },
            if typo_pairs.len() > 1 {
                "typos"
            } else {
                "typo"
            },
        ),
        (true, false) => {
            if case_pairs.len() > 1 {
                "cases".to_string()
            } else {
                "case".to_string()
            }
        }
        (false, true) => {
            if typo_pairs.len() > 1 {
                "typos".to_string()
            } else {
                "typo".to_string()
            }
        }
        (false, false) => String::new(),
    };

    let mut all = case_pairs;
    all.extend(typo_pairs);
    all.sort();
    let fixed_str = all
        .iter()
        .map(|(old, new)| format!("{} ⇒ {}", old, new))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "Fix field name {} in debian/tests/control ({}).",
        kind_str, fixed_str
    )
}

declare_detector! {
    name: "field-name-typo-in-tests-control",
    tags: ["field-name-typo-in-tests-control"],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_typo_fix() {
        let tmp = TempDir::new().unwrap();
        let tests = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests).unwrap();
        fs::write(
            tests.join("control"),
            "Tests: 4.08.1 ocaml-system\nDepends: @, ca-certificates\nRestrictions: isolation-container, allow-stderr\n\nTest: ocaml-system\nDepends: ocaml-nox\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Fix field name typo in debian/tests/control (Test ⇒ Tests)."
        );

        // Renamed in place; "Tests: ocaml-system" replaces "Test: ocaml-system".
        assert_eq!(
            fs::read_to_string(tests.join("control")).unwrap(),
            "Tests: 4.08.1 ocaml-system\nDepends: @, ca-certificates\nRestrictions: isolation-container, allow-stderr\n\nTests: ocaml-system\nDepends: ocaml-nox\n",
        );
    }

    #[test]
    fn test_case_fix() {
        let tmp = TempDir::new().unwrap();
        let tests = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests).unwrap();
        fs::write(tests.join("control"), "tests: some-test\nDepends: @\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Fix field name case in debian/tests/control (tests ⇒ Tests)."
        );

        assert_eq!(
            fs::read_to_string(tests.join("control")).unwrap(),
            "Tests: some-test\nDepends: @\n",
        );
    }

    #[test]
    fn test_multiple_fixes() {
        let tmp = TempDir::new().unwrap();
        let tests = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests).unwrap();
        fs::write(
            tests.join("control"),
            "tests: test1\nDepend: foo\n\nTest: test2\nrestrictions: needs-root\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        // Two case-only renames (tests→Tests, restrictions→Restrictions) and
        // two typos (Depend→Depends, Test→Tests).
        assert_eq!(
            result.description,
            "Fix field name cases and typos in debian/tests/control (Depend ⇒ Depends, Test ⇒ Tests, restrictions ⇒ Restrictions, tests ⇒ Tests).",
        );

        assert_eq!(
            fs::read_to_string(tests.join("control")).unwrap(),
            "Tests: test1\nDepends: foo\n\nTests: test2\nRestrictions: needs-root\n",
        );
    }

    #[test]
    fn test_no_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_needed() {
        let tmp = TempDir::new().unwrap();
        let tests = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests).unwrap();
        let original = "Tests: some-test\nDepends: @\nRestrictions: needs-root\n";
        fs::write(tests.join("control"), original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(tests.join("control")).unwrap(), original);
    }

    #[test]
    fn test_only_distance_one() {
        let tmp = TempDir::new().unwrap();
        let tests = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests).unwrap();
        fs::write(tests.join("control"), "Foo: some-test\nDepends: @\n").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
