use crate::{FixerError, FixerResult, LintianIssue};
use deb822_lossless::Deb822;
use debian_copyright::{pattern_depth, pattern_sort_key};
use std::path::Path;
use std::str::FromStr;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");
    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = std::fs::read_to_string(&copyright_path)?;

    let mut deb822 = match Deb822::from_str(&content) {
        Ok(d) => d,
        Err(_) => return Err(FixerError::NoChanges),
    };

    // Collect Files paragraphs with their indices, patterns, and depths
    let mut files_info: Vec<(usize, String, usize, usize)> = Vec::new();

    for (i, para) in deb822.paragraphs().enumerate() {
        if let Some(files_value) = para.get("Files") {
            let first_pattern = files_value
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            let depth = pattern_depth(&first_pattern);
            let line_num = para.line() + 1;
            files_info.push((i, first_pattern, depth, line_num));
        }
    }

    if files_info.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Detect which issues we have
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Check for wildcard not first (only if it exists and not at position 0)
    let wildcard_pos = files_info.iter().position(|(_, p, _, _)| p.trim() == "*");
    if let Some(pos) = wildcard_pos {
        if pos > 0 {
            let line_num = files_info[pos].3;
            let issue = LintianIssue::source_with_info(
                "global-files-wildcard-not-first-paragraph-in-dep5-copyright",
                vec![format!("[debian/copyright:{}]", line_num)],
            );

            if issue.should_fix(base_path) {
                fixed_issues.push(issue);
            } else {
                overridden_issues.push(issue);
            }
        }
    }

    // Check for out-of-order patterns (strictly less depth coming after greater depth)
    for i in 0..files_info.len() {
        for j in (i + 1)..files_info.len() {
            if files_info[j].2 < files_info[i].2 {
                let issue = LintianIssue::source_with_info(
                    "globbing-patterns-out-of-order",
                    vec![format!(
                        "{} {} [debian/copyright:{}]",
                        files_info[i].1, files_info[j].1, files_info[j].3
                    )],
                );

                if issue.should_fix(base_path) {
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
                break;
            }
        }
    }

    if fixed_issues.is_empty() {
        if overridden_issues.is_empty() {
            return Err(FixerError::NoChanges);
        }
        return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
    }

    // Use bubble sort to reorder - simple and correct, but only swap when strictly necessary
    let mut changed_order = true;
    while changed_order {
        changed_order = false;

        // Get current state of Files paragraphs
        let mut current_files: Vec<(usize, String, usize)> = Vec::new();
        for (i, para) in deb822.paragraphs().enumerate() {
            if let Some(files_value) = para.get("Files") {
                let first_pattern = files_value
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
                let depth = pattern_depth(&first_pattern);
                current_files.push((i, first_pattern, depth));
            }
        }

        // Find adjacent pairs that are out of order based on sort key
        for j in 0..current_files.len().saturating_sub(1) {
            let key_j = pattern_sort_key(&current_files[j].1, current_files[j].2);
            let key_j1 = pattern_sort_key(&current_files[j + 1].1, current_files[j + 1].2);

            // Only swap if j+1 should come before j (strictly less than)
            if key_j1 < key_j {
                // Swap these two paragraphs
                deb822.move_paragraph(current_files[j + 1].0, current_files[j].0);
                changed_order = true;
                break;
            }
        }
    }

    std::fs::write(&copyright_path, deb822.to_string())?;

    // Choose description based on which issues were fixed
    let description = if fixed_issues.iter().all(|i| {
        i.tag.as_deref() == Some("global-files-wildcard-not-first-paragraph-in-dep5-copyright")
    }) {
        "Make \"Files: *\" paragraph the first in the copyright file."
    } else {
        "Reorder Files paragraphs in debian/copyright by directory depth."
    };

    Ok(FixerResult::builder(description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "global-files-wildcard-not-first-paragraph-in-dep5-copyright",
    tags: ["global-files-wildcard-not-first-paragraph-in-dep5-copyright", "globbing-patterns-out-of-order"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_pattern_depth() {
        assert_eq!(pattern_depth("*"), 0);
        assert_eq!(pattern_depth("src/*"), 1);
        assert_eq!(pattern_depth("src/foo/*"), 2);
        assert_eq!(pattern_depth("a/b/c/d/*"), 4);
    }

    #[test]
    fn test_wildcard_detection() {
        // Test that we correctly identify wildcard paragraphs
        assert_eq!("*".trim(), "*");
        assert_eq!(" * ".trim(), "*");
        assert_eq!("* \n".trim(), "*");
    }

    #[test]
    fn test_files_paragraph_identification() {
        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2020 Author
License: MIT

Files: *
Copyright: 2020 Author
License: MIT
"#;

        let deb822 = Deb822::from_str(content).unwrap();
        let paragraphs: Vec<_> = deb822.paragraphs().collect();

        // First paragraph is header (no Files field)
        assert!(paragraphs[0].get("Format").is_some());
        assert!(paragraphs[0].get("Files").is_none());

        // Second paragraph has Files: src/*
        assert_eq!(paragraphs[1].get("Files").unwrap().trim(), "src/*");

        // Third paragraph has Files: *
        assert_eq!(paragraphs[2].get("Files").unwrap().trim(), "*");
    }

    #[test]
    fn test_wildcard_not_first() {
        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2020 Author
License: MIT

Files: *
Copyright: 2020 Author
License: MIT
"#;

        let deb822 = Deb822::from_str(content).unwrap();
        let paragraphs: Vec<_> = deb822.paragraphs().enumerate().collect();

        let mut first_files_index = None;
        let mut wildcard_index = None;
        let mut files_count = 0;

        for (i, para) in paragraphs.iter() {
            if let Some(files_value) = para.get("Files") {
                if first_files_index.is_none() {
                    first_files_index = Some(*i);
                }
                if files_value.trim() == "*" && files_count > 0 {
                    wildcard_index = Some(*i);
                    break;
                }
                files_count += 1;
            }
        }

        assert_eq!(first_files_index, Some(1)); // First Files paragraph at index 1
        assert_eq!(wildcard_index, Some(2)); // Wildcard paragraph at index 2
        assert_eq!(files_count, 1); // We stopped counting after finding wildcard
    }

    #[test]
    fn test_wildcard_already_first() {
        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2020 Author
License: MIT

Files: src/*
Copyright: 2020 Author
License: MIT
"#;

        let deb822 = Deb822::from_str(content).unwrap();
        let paragraphs: Vec<_> = deb822.paragraphs().enumerate().collect();

        let mut first_files_index = None;
        let mut wildcard_index = None;
        let mut files_count = 0;

        for (i, para) in paragraphs.iter() {
            if let Some(files_value) = para.get("Files") {
                if first_files_index.is_none() {
                    first_files_index = Some(*i);
                }
                if files_value.trim() == "*" && files_count > 0 {
                    wildcard_index = Some(*i);
                    break;
                }
                files_count += 1;
            }
        }

        assert_eq!(first_files_index, Some(1));
        assert_eq!(wildcard_index, None); // No wildcard found after first Files paragraph
    }

    #[test]
    fn test_no_wildcard_paragraph() {
        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2020 Author
License: MIT

Files: tests/*
Copyright: 2020 Author
License: MIT
"#;

        let deb822 = Deb822::from_str(content).unwrap();
        let paragraphs: Vec<_> = deb822.paragraphs().enumerate().collect();

        let mut wildcard_found = false;
        for (_, para) in paragraphs.iter() {
            if let Some(files_value) = para.get("Files") {
                if files_value.trim() == "*" {
                    wildcard_found = true;
                    break;
                }
            }
        }

        assert!(!wildcard_found);
    }

    #[test]
    fn test_integration_with_tempdir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2020 Author
License: MIT

Files: *
Copyright: 2020 Author
License: MIT
"#;

        fs::write(debian_dir.join("copyright"), content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(debian_dir.join("copyright")).unwrap();
        let deb822 = Deb822::from_str(&updated_content).unwrap();
        let paragraphs: Vec<_> = deb822.paragraphs().collect();

        // Check that Files: * is now the first Files paragraph (index 1)
        assert_eq!(paragraphs[1].get("Files").unwrap().trim(), "*");
        assert_eq!(paragraphs[2].get("Files").unwrap().trim(), "src/*");
    }

    #[test]
    fn test_out_of_order_patterns() {
        let temp_dir = tempfile::tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/foo/bar/*
Copyright: 2020 Author
License: MIT

Files: src/*
Copyright: 2020 Another
License: GPL-2
"#;

        fs::write(debian_dir.join("copyright"), content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok());
        let result = result.unwrap();

        // Should detect globbing-patterns-out-of-order
        assert!(result
            .fixed_lintian_issues
            .iter()
            .any(|i| i.tag.as_deref() == Some("globbing-patterns-out-of-order")));

        let updated_content = fs::read_to_string(debian_dir.join("copyright")).unwrap();
        let deb822 = Deb822::from_str(&updated_content).unwrap();
        let paragraphs: Vec<_> = deb822.paragraphs().collect();

        // Check that src/* (depth 1) is now before src/foo/bar/* (depth 3)
        assert_eq!(paragraphs[1].get("Files").unwrap().trim(), "src/*");
        assert_eq!(paragraphs[2].get("Files").unwrap().trim(), "src/foo/bar/*");
    }

    #[test]
    fn test_both_issues_together() {
        let temp_dir = tempfile::tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/foo/*
Copyright: 2020 Author
License: MIT

Files: *
Copyright: 2020 Generic
License: GPL-2

Files: src/*
Copyright: 2020 Another
License: Apache-2.0
"#;

        fs::write(debian_dir.join("copyright"), content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok());
        let result = result.unwrap();

        // Should detect both issues
        assert!(result.fixed_lintian_issues.iter().any(|i| i.tag.as_deref()
            == Some("global-files-wildcard-not-first-paragraph-in-dep5-copyright")));
        assert!(result
            .fixed_lintian_issues
            .iter()
            .any(|i| i.tag.as_deref() == Some("globbing-patterns-out-of-order")));

        let updated_content = fs::read_to_string(debian_dir.join("copyright")).unwrap();
        let deb822 = Deb822::from_str(&updated_content).unwrap();
        let paragraphs: Vec<_> = deb822.paragraphs().collect();

        // Check correct order: * (0), src/* (1), src/foo/* (2)
        assert_eq!(paragraphs[1].get("Files").unwrap().trim(), "*");
        assert_eq!(paragraphs[2].get("Files").unwrap().trim(), "src/*");
        assert_eq!(paragraphs[3].get("Files").unwrap().trim(), "src/foo/*");
    }

    #[test]
    fn test_already_sorted() {
        let temp_dir = tempfile::tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2020 Generic
License: GPL-2

Files: src/*
Copyright: 2020 Another
License: Apache-2.0

Files: src/foo/*
Copyright: 2020 Author
License: MIT
"#;

        fs::write(debian_dir.join("copyright"), content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_debian_pattern_stays_last() {
        let temp_dir = tempfile::tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // debian/* should stay at the end even though it has same depth as src/*
        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/foo/*
Copyright: 2020 Author
License: MIT

Files: debian/*
Copyright: 2020 Debian
License: GPL-2

Files: src/*
Copyright: 2020 Another
License: Apache-2.0
"#;

        fs::write(debian_dir.join("copyright"), content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(debian_dir.join("copyright")).unwrap();
        let deb822 = Deb822::from_str(&updated_content).unwrap();
        let paragraphs: Vec<_> = deb822.paragraphs().collect();

        // Check order: src/* (depth 1), src/foo/* (depth 2), debian/* (depth 1 but last)
        assert_eq!(paragraphs[1].get("Files").unwrap().trim(), "src/*");
        assert_eq!(paragraphs[2].get("Files").unwrap().trim(), "src/foo/*");
        assert_eq!(paragraphs[3].get("Files").unwrap().trim(), "debian/*");
    }

    #[test]
    fn test_debian_pattern_already_last() {
        let temp_dir = tempfile::tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // debian/* is already at the end - should not change
        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2020 Generic
License: GPL-2

Files: src/*
Copyright: 2020 Another
License: Apache-2.0

Files: debian/*
Copyright: 2020 Debian
License: GPL-2
"#;

        fs::write(debian_dir.join("copyright"), content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
