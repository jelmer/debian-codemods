use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::editor::check_generated_file;
use std::fs;
use std::path::Path;

/// Strip trailing whitespace from a line
fn strip_whitespace(line: &[u8], strip_tabs: bool) -> Vec<u8> {
    let mut result = line.to_vec();

    // Find the newline
    if let Some(newline_pos) = result.iter().position(|&b| b == b'\n') {
        // Strip backwards from the newline
        let mut end = newline_pos;
        while end > 0 {
            let prev = result[end - 1];
            if prev == b' ' || (strip_tabs && prev == b'\t') {
                end -= 1;
            } else {
                break;
            }
        }

        // Keep content up to end, then add newline
        result.truncate(end);
        result.push(b'\n');
    }

    result
}

struct FileStripResult {
    fixed_issues: Vec<LintianIssue>,
    overridden_issues: Vec<LintianIssue>,
}

/// Strip whitespace from a file, checking should_fix() for each line
fn file_strip_whitespace(
    base_path: &Path,
    file_path: &Path,
    relative_path: &str,
    strip_tabs: bool,
    strip_trailing_empty_lines: bool,
    delete_new_empty_line: bool,
) -> Result<FileStripResult, std::io::Error> {
    let content = match fs::read(file_path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileStripResult {
                fixed_issues: Vec::new(),
                overridden_issues: Vec::new(),
            })
        }
        Err(e) => return Err(e),
    };

    let mut lines_to_fix = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // First pass: find lines with trailing whitespace and check if they should be fixed
    for (line_idx, line) in content.split_inclusive(|&b| b == b'\n').enumerate() {
        let newline = strip_whitespace(line, strip_tabs);
        if newline != line {
            let line_num = line_idx + 1;
            let issue = LintianIssue::source_with_info(
                "trailing-whitespace",
                vec![format!("[{}:{}]", relative_path, line_num)],
            );

            if issue.should_fix(base_path) {
                lines_to_fix.push(line_idx);
                fixed_issues.push(issue);
            } else {
                overridden_issues.push(issue);
            }
        }
    }

    // Check if we need to strip trailing empty lines
    if strip_trailing_empty_lines {
        let lines: Vec<&[u8]> = content.split_inclusive(|&b| b == b'\n').collect();
        let trailing_empty_count = lines.iter().rev().take_while(|l| **l == b"\n").count();
        if trailing_empty_count > 0 {
            let issue = LintianIssue::source_with_info(
                "trailing-whitespace",
                vec![format!("[{}:EOF]", relative_path)],
            );

            if issue.should_fix(base_path) {
                lines_to_fix.push(usize::MAX); // Special marker for trailing empty lines
                fixed_issues.push(issue);
            } else {
                overridden_issues.push(issue);
            }
        }
    }

    if lines_to_fix.is_empty() {
        return Ok(FileStripResult {
            fixed_issues,
            overridden_issues,
        });
    }

    // Second pass: actually strip whitespace from lines that should be fixed
    let mut newlines = Vec::new();
    for (line_idx, line) in content.split_inclusive(|&b| b == b'\n').enumerate() {
        if lines_to_fix.contains(&line_idx) {
            let newline = strip_whitespace(line, strip_tabs);
            if newline == b"\n" && delete_new_empty_line {
                continue;
            }
            newlines.push(newline);
        } else {
            newlines.push(line.to_vec());
        }
    }

    // Strip trailing empty lines (only if we decided to fix them)
    if strip_trailing_empty_lines && lines_to_fix.contains(&usize::MAX) {
        while !newlines.is_empty() && newlines.last().is_some_and(|l| l == b"\n") {
            newlines.pop();
        }
    }

    let output: Vec<u8> = newlines.into_iter().flatten().collect();
    fs::write(file_path, output)?;

    Ok(FileStripResult {
        fixed_issues,
        overridden_issues,
    })
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Process debian/changelog
    let changelog_path = base_path.join("debian/changelog");
    if changelog_path.exists() {
        let result = file_strip_whitespace(
            base_path,
            &changelog_path,
            "debian/changelog",
            true,
            true,
            false,
        )?;
        fixed_issues.extend(result.fixed_issues);
        overridden_issues.extend(result.overridden_issues);
    }

    // Process debian/rules
    let rules_path = base_path.join("debian/rules");
    if rules_path.exists() {
        // For debian/rules, don't strip tabs
        let result =
            file_strip_whitespace(base_path, &rules_path, "debian/rules", false, true, false)?;
        fixed_issues.extend(result.fixed_issues);
        overridden_issues.extend(result.overridden_issues);
    }

    // Process debian/control
    let control_path = base_path.join("debian/control");
    if control_path.exists() {
        match check_generated_file(&control_path) {
            Err(_generated_file) => {
                // Control file is generated, process control.* files instead
                let debian_dir = base_path.join("debian");
                if let Ok(entries) = fs::read_dir(&debian_dir) {
                    for entry in entries.filter_map(Result::ok) {
                        let file_name = entry.file_name();
                        let name = file_name.to_string_lossy();

                        if !name.starts_with("control.")
                            || name.ends_with('~')
                            || name.ends_with(".m4")
                        {
                            continue;
                        }

                        let relative_path = format!("debian/{}", name);
                        let result = file_strip_whitespace(
                            base_path,
                            &entry.path(),
                            &relative_path,
                            true,
                            true,
                            true,
                        )?;
                        fixed_issues.extend(result.fixed_issues);
                        overridden_issues.extend(result.overridden_issues);
                    }

                    // Also process the generated control file if we made changes to control.* files
                    if !fixed_issues.is_empty() {
                        file_strip_whitespace(
                            base_path,
                            &control_path,
                            "debian/control",
                            true,
                            true,
                            true,
                        )?;
                    }
                }
            }
            Ok(()) => {
                // Control file is not generated, process it directly
                let result = file_strip_whitespace(
                    base_path,
                    &control_path,
                    "debian/control",
                    true,
                    true,
                    true,
                )?;
                fixed_issues.extend(result.fixed_issues);
                overridden_issues.extend(result.overridden_issues);
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    Ok(FixerResult::builder("Trim trailing whitespace.")
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "file-contains-trailing-whitespace",
    tags: ["trailing-whitespace"],
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
    fn test_strip_whitespace_spaces() {
        let line = b"hello  \n";
        let result = strip_whitespace(line, true);
        assert_eq!(result, b"hello\n");
    }

    #[test]
    fn test_strip_whitespace_tabs() {
        let line = b"hello\t\n";
        let result = strip_whitespace(line, true);
        assert_eq!(result, b"hello\n");
    }

    #[test]
    fn test_strip_whitespace_no_tabs() {
        let line = b"hello\t\n";
        let result = strip_whitespace(line, false);
        assert_eq!(result, b"hello\t\n");
    }

    #[test]
    fn test_strip_whitespace_mixed() {
        let line = b"hello \t \n";
        let result = strip_whitespace(line, true);
        assert_eq!(result, b"hello\n");
    }

    #[test]
    fn test_file_strip_whitespace_control() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = b"Source: lintian-brush  \n\nPackage: lintian-brush\nDescription: Testing\n Test test\t\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read(&control_path).unwrap();
        assert_eq!(
            updated_content,
            b"Source: lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n"
        );
    }

    #[test]
    fn test_no_changes_needed() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content =
            b"Source: lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_strip_trailing_empty_lines() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = b"Source: test\n\n\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read(&control_path).unwrap();
        assert_eq!(updated_content, b"Source: test\n");
    }
}
