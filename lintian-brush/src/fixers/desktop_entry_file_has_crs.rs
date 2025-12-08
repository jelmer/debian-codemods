use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let debian_dir = base_path.join("debian");

    if !debian_dir.exists() {
        return Err(FixerError::NoChanges);
    }

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut made_changes = false;

    // Find all .desktop files in the debian directory
    let entries = fs::read_dir(&debian_dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Only process .desktop files
        if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
            continue;
        }

        // Read the file as bytes to preserve exact binary content
        let content = fs::read(&path)?;

        // Check if there are any CR characters
        if !content.contains(&b'\r') {
            continue;
        }

        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| FixerError::Other("Invalid filename".to_string()))?;

        // Build the installed path (assuming debian/<package>.desktop -> /usr/share/applications/<package>.desktop)
        let installed_path = format!("usr/share/applications/{}", filename);

        // Process line by line to track which lines have CRs
        let lines: Vec<&[u8]> = content.split(|&b| b == b'\n').collect();
        let mut lines_to_fix = Vec::new();

        for (line_idx, line) in lines.iter().enumerate() {
            if line.contains(&b'\r') {
                let line_no = line_idx + 1;
                let issue = LintianIssue::source_with_info(
                    "desktop-entry-file-has-crs",
                    vec![format!("[{}:{}]", installed_path, line_no)],
                );

                if issue.should_fix(base_path) {
                    lines_to_fix.push(line_idx);
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
            }
        }

        // Only modify the file if there are lines to fix
        if !lines_to_fix.is_empty() {
            // Build new content, removing CRs only from lines that should be fixed
            let mut new_content = Vec::new();
            for (line_idx, line) in lines.iter().enumerate() {
                if lines_to_fix.contains(&line_idx) {
                    // Remove CRs from this line
                    let cleaned: Vec<u8> = line.iter().copied().filter(|&b| b != b'\r').collect();
                    new_content.extend_from_slice(&cleaned);
                } else {
                    // Keep the line as-is (including any CRs if they exist and are overridden)
                    new_content.extend_from_slice(line);
                }

                // Add back the newline separator (except after the last line if it didn't have one)
                if line_idx < lines.len() - 1 {
                    new_content.push(b'\n');
                } else if content.ends_with(b"\n") {
                    new_content.push(b'\n');
                }
            }

            fs::write(&path, new_content)?;
            made_changes = true;
        }
    }

    if !made_changes {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    Ok(FixerResult::builder("Remove CRs from desktop files.")
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .certainty(crate::Certainty::Certain)
        .build())
}

declare_fixer! {
    name: "desktop-entry-file-has-crs",
    tags: ["desktop-entry-file-has-crs"],
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
    fn test_remove_crs_from_desktop_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let desktop_path = debian_dir.join("test.desktop");
        // Write content with CRLF line endings
        fs::write(
            &desktop_path,
            b"[Desktop Entry]\r\nType=Application\r\nName=Test\r\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove CRs from desktop files.");
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that CRs were removed
        let content = fs::read(&desktop_path).unwrap();
        assert!(!content.contains(&b'\r'));
        assert_eq!(content, b"[Desktop Entry]\nType=Application\nName=Test\n");
    }

    #[test]
    fn test_multiple_desktop_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create multiple desktop files, some with CRs, some without
        let desktop1_path = debian_dir.join("app1.desktop");
        let desktop2_path = debian_dir.join("app2.desktop");
        let non_desktop_path = debian_dir.join("control");

        fs::write(&desktop1_path, b"[Desktop Entry]\r\nType=Application\r\n").unwrap();
        fs::write(&desktop2_path, b"[Desktop Entry]\nType=Service\n").unwrap(); // No CRs
        fs::write(&non_desktop_path, b"Source: test\r\n").unwrap(); // Non-desktop file with CRs

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        // Check that CRs were removed from desktop files only
        let content1 = fs::read(&desktop1_path).unwrap();
        assert!(!content1.contains(&b'\r'));

        let content2 = fs::read(&desktop2_path).unwrap();
        assert!(!content2.contains(&b'\r')); // Already didn't have CRs

        let control_content = fs::read(&non_desktop_path).unwrap();
        assert!(control_content.contains(&b'\r')); // Should be unchanged
    }

    #[test]
    fn test_no_desktop_files_with_crs() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let desktop_path = debian_dir.join("test.desktop");
        // Write content with LF line endings only
        fs::write(&desktop_path, b"[Desktop Entry]\nType=Application\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_desktop_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Only non-desktop files
        fs::write(debian_dir.join("control"), b"Source: test\r\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
