use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use deb822_lossless::Deb822;
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

    // Collect all paragraphs with their indices
    let paragraphs: Vec<_> = deb822.paragraphs().enumerate().collect();

    // Find Files paragraphs
    let mut first_files_index = None;
    let mut wildcard_index = None;
    let mut wildcard_line = None;
    let mut files_count = 0;

    for (i, para) in paragraphs.iter() {
        if let Some(files_value) = para.get("Files") {
            if first_files_index.is_none() {
                first_files_index = Some(*i);
            }
            if files_value.trim() == "*" && files_count > 0 {
                wildcard_index = Some(*i);
                wildcard_line = Some(para.line() + 1);
                break;
            }
            files_count += 1;
        }
    }

    // If we found a "Files: *" paragraph that's not the first Files paragraph, move it
    if let (Some(wildcard_idx), Some(first_idx), Some(line_num)) = (wildcard_index, first_files_index, wildcard_line) {
        let issue = LintianIssue::source_with_info(
            "global-files-wildcard-not-first-paragraph-in-dep5-copyright",
            vec![format!("[debian/copyright:{}]", line_num)],
        );

        if !issue.should_fix(base_path) {
            return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
        }

        // Move the wildcard paragraph to the first Files position
        deb822.move_paragraph(wildcard_idx, first_idx);

        // Write the updated copyright file
        std::fs::write(&copyright_path, deb822.to_string())?;

        Ok(FixerResult::builder(
            "Make \"Files: *\" paragraph the first in the copyright file",
        )
        .fixed_issues(vec![issue])
        .build())
    } else {
        return Err(FixerError::NoChanges);
    }
}

declare_fixer! {
    name: "global-files-wildcard-not-first-paragraph-in-dep5-copyright",
    tags: ["global-files-wildcard-not-first-paragraph-in-dep5-copyright"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
}
