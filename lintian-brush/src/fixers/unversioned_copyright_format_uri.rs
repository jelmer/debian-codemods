use crate::{FixerError, FixerResult};
use regex::Regex;
use std::fs;

declare_fixer! {
    name: "unversioned-copyright-format-uri",
    tags: ["unversioned-copyright-format-uri"],
    // Must run after URI is converted to https and before updating to latest version
    after: ["copyright-format-uri"],
    before: ["out-of-date-copyright-format-uri"],
    apply: |basedir, _package, _version, _preferences| {
        let copyright_path = basedir.join("debian").join("copyright");

        if !copyright_path.exists() {
            return Err(FixerError::NoChanges);
        }

        let content = fs::read(&copyright_path)?;

        // Check if the file is empty
        if content.is_empty() {
            return Err(FixerError::NoChanges);
        }

        // Find the first line
        let first_line_end = content.iter().position(|&b| b == b'\n').unwrap_or(content.len());
        let first_line = &content[..first_line_end];

        // Regular expression to match Format or Format-Specification lines
        let format_regex = Regex::new(r"^(Format|Format-Specification):\s*(.*)$").unwrap();

        // The expected format URI
        const EXPECTED_URL: &[u8] = b"https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/";

        // Convert first line to string for regex matching
        let first_line_str = String::from_utf8_lossy(first_line);

        if let Some(captures) = format_regex.captures(&first_line_str) {
            let field_name = captures.get(1).unwrap().as_str();
            let url = captures.get(2).unwrap().as_str().trim();

            // Check if we need to make changes
            if field_name == "Format" && url.trim_end_matches('/').as_bytes() == EXPECTED_URL.strip_suffix(b"/").unwrap_or(EXPECTED_URL) {
                // Already correct
                return Err(FixerError::NoChanges);
            }

            let issue = crate::LintianIssue::source_with_info(
                "unversioned-copyright-format-uri",
                vec!["debian/copyright:1".to_string()],
            );

            if !issue.should_fix(basedir) {
                return Err(FixerError::NoChanges);
            }

            // Build the new content
            let mut new_content = Vec::new();
            new_content.extend_from_slice(b"Format: ");
            new_content.extend_from_slice(EXPECTED_URL);
            new_content.push(b'\n');

            // Add the rest of the file (skipping the original first line)
            if first_line_end < content.len() {
                new_content.extend_from_slice(&content[first_line_end + 1..]);
            }

            // Write the updated content back
            fs::write(&copyright_path, new_content)?;

            Ok(FixerResult::builder("Use versioned copyright format URI.")
                .fixed_issues(vec![issue])
                .build())
        } else {
            // No Format or Format-Specification field found on first line
            Err(FixerError::NoChanges)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_updates_format_specification_field() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let copyright_content = b"Format-Specification: http://svn.debian.org/wsvn/dep/web/deps/dep5.mdwn?op=file&rev=59
Upstream-Name: test-package

Files: *
Copyright: 2023 Test Author
License: GPL-2+
";

        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, copyright_content).unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        // Verify the change
        let updated_content = fs::read(&copyright_path).unwrap();
        assert!(updated_content.starts_with(
            b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n"
        ));
        assert!(updated_content
            .windows(b"Format-Specification:".len())
            .all(|w| w != b"Format-Specification:"));
    }

    #[test]
    fn test_updates_unversioned_format_field() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let copyright_content =
            b"Format: http://www.debian.org/doc/packaging-manuals/copyright-format/
Upstream-Name: test-package

Files: *
Copyright: 2023 Test Author
License: GPL-2+
";

        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, copyright_content).unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        // Verify the change
        let updated_content = fs::read(&copyright_path).unwrap();
        assert!(updated_content.starts_with(
            b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n"
        ));
    }

    #[test]
    fn test_no_change_when_format_correct() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let copyright_content =
            b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: test-package

Files: *
Copyright: 2023 Test Author
License: GPL-2+
";

        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, copyright_content).unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_format_correct_without_trailing_slash() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let copyright_content =
            b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0
Upstream-Name: test-package

Files: *
Copyright: 2023 Test Author
License: GPL-2+
";

        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, copyright_content).unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_copyright_file() {
        let temp_dir = TempDir::new().unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_empty_copyright_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, b"").unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
