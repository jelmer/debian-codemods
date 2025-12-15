use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use deb822_lossless::{Deb822, Paragraph};
use log::warn;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::str::FromStr;

const VALID_FIELD_NAMES: &[&str] = &[
    "Files",
    "License",
    "Copyright",
    "Comment",
    "Upstream-Name",
    "Format",
    "Upstream-Contact",
    "Source",
    "Upstream",
    "Contact",
    "Name",
];

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");

    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&copyright_path)?;
    let deb822 = Deb822::from_str(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/copyright: {:?}", e)))?;

    let valid_fields: HashSet<&str> = VALID_FIELD_NAMES.iter().copied().collect();
    let mut case_fixed = Vec::new();
    let mut typo_fixed = Vec::new();

    for mut paragraph in deb822.paragraphs() {
        let field_names: Vec<String> = paragraph.keys().collect();

        for field_name in field_names {
            if valid_fields.contains(field_name.as_str()) {
                continue;
            }

            // Try to handle X- prefix fields
            if let Some(fixed) = try_fix_x_prefix(&mut paragraph, &field_name, &valid_fields)? {
                typo_fixed.push(fixed);
                continue;
            }

            // Try to fix with Levenshtein distance
            if let Some(fixed) = try_fix_levenshtein(&mut paragraph, &field_name)? {
                if fixed.1.eq_ignore_ascii_case(&fixed.0) {
                    case_fixed.push(fixed);
                } else {
                    typo_fixed.push(fixed);
                }
            }
        }
    }

    if case_fixed.is_empty() && typo_fixed.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Create LintianIssue for each typo fix (not case fixes)
    let mut fixed_issues = Vec::new();
    for (old_name, _new_name) in &typo_fixed {
        let issue = LintianIssue::source_with_info(
            "field-name-typo-in-dep5-copyright",
            vec![old_name.clone()],
        );

        if !issue.should_fix(base_path) {
            // If any issue is overridden, skip all changes
            return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
        }
        fixed_issues.push(issue);
    }

    fs::write(&copyright_path, deb822.to_string())?;

    let kind = build_kind_string(&case_fixed, &typo_fixed);
    let fixed_str = build_fixed_string(&case_fixed, &typo_fixed);

    Ok(FixerResult::builder(format!(
        "Fix field name {} in debian/copyright ({}).",
        kind, fixed_str
    ))
    .fixed_issues(fixed_issues)
    .build())
}

fn try_fix_x_prefix(
    paragraph: &mut Paragraph,
    field_name: &str,
    valid_fields: &HashSet<&str>,
) -> Result<Option<(String, String)>, FixerError> {
    if !field_name.starts_with("X-") {
        return Ok(None);
    }

    let without_prefix = &field_name[2..];
    if !valid_fields.contains(without_prefix) {
        return Ok(None);
    }

    if paragraph.get(without_prefix).is_some() {
        warn!("Both {} and {} exist.", field_name, without_prefix);
        return Ok(None);
    }

    let value = paragraph.get(field_name).ok_or(FixerError::NoChanges)?;
    let value_str = value.to_string();

    paragraph.remove(field_name);
    paragraph.insert(without_prefix, &value_str);

    Ok(Some((field_name.to_string(), without_prefix.to_string())))
}

fn try_fix_levenshtein(
    paragraph: &mut Paragraph,
    field_name: &str,
) -> Result<Option<(String, String)>, FixerError> {
    for &valid_field in VALID_FIELD_NAMES {
        if strsim::levenshtein(field_name, valid_field) != 1 {
            continue;
        }

        // Check if target field already exists
        if let Some(existing_value) = paragraph.get(valid_field) {
            if !valid_field.eq_ignore_ascii_case(field_name) {
                warn!(
                    "Found typo ({} ⇒ {}), but {} already exists",
                    field_name, valid_field, valid_field
                );
                return Ok(None);
            }

            // If it's just a case difference, check if values differ
            let value = paragraph.get(field_name).ok_or(FixerError::NoChanges)?;
            if value != existing_value {
                warn!(
                    "Found typo ({} ⇒ {}), but {} already exists",
                    field_name, valid_field, valid_field
                );
                return Ok(None);
            }
        }

        let value = paragraph.get(field_name).ok_or(FixerError::NoChanges)?;
        let value_str = value.to_string();

        paragraph.remove(field_name);
        paragraph.insert(valid_field, &value_str);

        return Ok(Some((field_name.to_string(), valid_field.to_string())));
    }

    Ok(None)
}

fn build_kind_string(case_fixed: &[(String, String)], typo_fixed: &[(String, String)]) -> String {
    match (!case_fixed.is_empty(), !typo_fixed.is_empty()) {
        (true, true) => format!(
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
        ),
        (true, false) => {
            if case_fixed.len() > 1 {
                "cases".to_string()
            } else {
                "case".to_string()
            }
        }
        (false, true) => {
            if typo_fixed.len() > 1 {
                "typos".to_string()
            } else {
                "typo".to_string()
            }
        }
        (false, false) => String::new(),
    }
}

fn build_fixed_string(case_fixed: &[(String, String)], typo_fixed: &[(String, String)]) -> String {
    let mut all_fixes = case_fixed.to_vec();
    all_fixes.extend(typo_fixed.iter().cloned());
    all_fixes.sort();

    all_fixes
        .iter()
        .map(|(old, new)| format!("{} ⇒ {}", old, new))
        .collect::<Vec<_>>()
        .join(", ")
}

declare_fixer! {
    name: "field-name-typo-in-dep5-copyright",
    tags: ["field-name-typo-in-dep5-copyright"],
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
    fn test_simple_typo() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\n\nFile: *\nCopyright:\n 2008-2017 Somebody\nLicense: GPL-2+\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Fix field name typo in debian/copyright (File ⇒ Files)."
        );
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag,
            Some("field-name-typo-in-dep5-copyright".to_string())
        );

        let content = fs::read_to_string(debian_dir.join("copyright")).unwrap();
        assert!(content.contains("Files: *"));
        assert!(!content.contains("File: *"));
    }

    #[test]
    fn test_case_fix() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-name: lintrian\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: GPL-2+\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Fix field name case in debian/copyright (Upstream-name ⇒ Upstream-Name)."
        );
        // Case fixes don't get lintian tags
        assert_eq!(result.fixed_lintian_issues.len(), 0);

        let content = fs::read_to_string(debian_dir.join("copyright")).unwrap();
        assert!(content.contains("Upstream-Name: lintrian"));
        assert!(!content.contains("Upstream-name: lintrian"));
    }

    #[test]
    fn test_x_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: GPL-2+\nX-Comment: blah\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Fix field name typo in debian/copyright (X-Comment ⇒ Comment)."
        );
        assert_eq!(result.fixed_lintian_issues.len(), 1);

        let content = fs::read_to_string(debian_dir.join("copyright")).unwrap();
        assert!(content.contains("Comment: blah"));
        assert!(!content.contains("X-Comment: blah"));
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
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: GPL-2+\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
