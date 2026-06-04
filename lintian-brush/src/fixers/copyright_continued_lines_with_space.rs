use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const EXPECTED_HEADER: &[u8] =
    b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0";
const UNICODE_LINE_BREAK: &[u8] = "\u{2028}".as_bytes();
const UNICODE_PARAGRAPH_SEPARATOR: &[u8] = "\u{2029}".as_bytes();

fn is_whitespace(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
}

fn whitespace_prefix_length(line: &[u8]) -> usize {
    line.iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .count()
}

fn value_offset(line: &[u8]) -> Option<usize> {
    if line.iter().all(|&b| is_whitespace(b)) {
        return None;
    }
    if line.starts_with(b"#") {
        return None;
    }
    if line.starts_with(b"\t") || line.starts_with(b" ") {
        return Some(whitespace_prefix_length(line));
    }
    line.iter()
        .position(|&b| b == b':')
        .map(|colon_pos| colon_pos + 1 + whitespace_prefix_length(&line[colon_pos + 1..]))
}

fn split_bytes(data: &[u8], separator: &[u8]) -> Vec<Vec<u8>> {
    let mut result = Vec::new();
    let mut current = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if i + separator.len() <= data.len() && &data[i..i + separator.len()] == separator {
            result.push(current);
            current = Vec::new();
            i += separator.len();
        } else {
            current.push(data[i]);
            i += 1;
        }
    }
    result.push(current);
    result
}

fn join_bytes(parts: Vec<Vec<u8>>, separator: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    for (i, part) in parts.into_iter().enumerate() {
        if i > 0 {
            result.extend_from_slice(separator);
        }
        result.extend(part);
    }
    result
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let content = match ws.read_file(&copyright_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let mut lines = content.split_inclusive(|&b| b == b'\n').peekable();
    let Some(first_line) = lines.peek().copied() else {
        return Ok(Vec::new());
    };

    let mut trimmed = first_line;
    while trimmed.last().is_some_and(|&b| is_whitespace(b)) {
        trimmed = &trimmed[..trimmed.len() - 1];
    }
    while trimmed.last() == Some(&b'/') {
        trimmed = &trimmed[..trimmed.len() - 1];
    }
    if trimmed != EXPECTED_HEADER {
        return Ok(Vec::new());
    }

    let mut new_lines: Vec<Vec<u8>> = Vec::new();
    let mut unicode_linebreaks_replaced = false;
    let mut prev_value_offset: Option<usize> = None;
    let mut tab_issues: Vec<LintianIssue> = Vec::new();
    let mut line_number = 0usize;

    for line in lines {
        line_number += 1;
        let mut line = line.to_vec();

        if line.starts_with(b"\t") {
            tab_issues.push(LintianIssue::source_with_info(
                "tab-in-license-text",
                Visibility::Warning,
                vec![format!("debian/copyright:{}", line_number)],
            ));

            let make_option = |prefix: &[u8], skip: usize| {
                let mut v = prefix.to_vec();
                if line.len() > skip {
                    v.extend_from_slice(&line[skip..]);
                }
                v
            };
            let options = [
                make_option(b" \t", 1),
                make_option(b" \t", 2),
                make_option(&[b' '; 8], 1),
            ];
            // Prefer one of the options that lines up with the previous line's
            // indentation. When none matches, replace the leading tab with a
            // single space rather than space+tab, so consistently tab-indented
            // text does not end up with every line starting with " \t" (#966631).
            line = options
                .into_iter()
                .find(|opt| value_offset(opt) == prev_value_offset)
                .unwrap_or_else(|| make_option(b" ", 1));
        }

        if line
            .windows(UNICODE_PARAGRAPH_SEPARATOR.len())
            .any(|w| w == UNICODE_PARAGRAPH_SEPARATOR)
        {
            let parts = split_bytes(&line, UNICODE_PARAGRAPH_SEPARATOR);
            let separator = [UNICODE_LINE_BREAK, UNICODE_LINE_BREAK].concat();
            line = join_bytes(parts, &separator);
        }

        if line
            .windows(UNICODE_LINE_BREAK.len())
            .any(|w| w == UNICODE_LINE_BREAK)
        {
            unicode_linebreaks_replaced = true;
            let parts = split_bytes(&line, UNICODE_LINE_BREAK);
            let new_parts: Vec<_> = parts
                .into_iter()
                .enumerate()
                .map(|(i, part)| {
                    let content = if part.is_empty() { b"." } else { &part[..] };
                    if i == 0 {
                        content.to_vec()
                    } else {
                        [b" ", content].concat()
                    }
                })
                .collect();
            line = join_bytes(new_parts, b"\n");
        }

        prev_value_offset = value_offset(&line);
        new_lines.push(line);
    }

    if tab_issues.is_empty() && !unicode_linebreaks_replaced {
        return Ok(Vec::new());
    }

    let new_content: Vec<u8> = new_lines.into_iter().flatten().collect();

    let mut label = "debian/copyright: ".to_string();
    if !tab_issues.is_empty() {
        label.push_str("use spaces rather than tabs to start continuation lines");
        if unicode_linebreaks_replaced {
            label.push_str(", ");
        }
    }
    if unicode_linebreaks_replaced {
        label.push_str("replace unicode linebreaks with regular linebreaks");
    }
    label.push('.');

    let mut problem_parts: Vec<&str> = Vec::new();
    if !tab_issues.is_empty() {
        problem_parts.push("contains tab-indented continuation lines");
    }
    if unicode_linebreaks_replaced {
        problem_parts.push("contains unicode line breaks");
    }
    let description = format!("debian/copyright {}.", problem_parts.join(" and "));

    let action = Action::Filesystem(FilesystemAction::Write {
        file: copyright_rel,
        content: new_content,
    });

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    if tab_issues.is_empty() {
        diagnostics.push(Diagnostic::untagged(
            description.clone(),
            label.clone(),
            vec![action],
        ));
    } else {
        for (i, issue) in tab_issues.into_iter().enumerate() {
            let actions = if i == 0 {
                vec![action.clone()]
            } else {
                Vec::new()
            };
            diagnostics.push(Diagnostic::with_actions(
                issue,
                description.clone(),
                label.clone(),
                actions,
            ));
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "copyright-continued-lines-with-space",
    tags: ["tab-in-license-text"],
    triggers: [debian_workspace::Trigger::File("debian/copyright")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(v.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_replace_tabs() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let copyright = debian.join("copyright");
        fs::write(
            &copyright,
            b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nLicense: GPL-3+\n\tThis is a continuation line\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "debian/copyright: use spaces rather than tabs to start continuation lines."
        );
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag,
            Some("tab-in-license-text".to_string())
        );
        assert_eq!(
            fs::read(&copyright).unwrap(),
            b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nLicense: GPL-3+\n This is a continuation line\n",
        );
    }

    #[test]
    fn test_no_changes() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nLicense: GPL-3+\n This is a continuation line\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_not_machine_readable() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            b"This is a regular copyright file\nCopyright (c) 2024 Someone\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_unicode_linebreaks() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let copyright = debian.join("copyright");
        let mut content = b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nLicense: GPL-3+\n Line one".to_vec();
        content.extend_from_slice(UNICODE_LINE_BREAK);
        content.extend_from_slice(b"Line two\n");
        fs::write(&copyright, &content).unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read(&copyright).unwrap(),
            b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nLicense: GPL-3+\n Line one\n Line two\n",
        );
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
