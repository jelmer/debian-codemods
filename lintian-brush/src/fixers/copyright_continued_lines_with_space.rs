use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::path::Path;

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
    // Look for key: value
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

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");

    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read(&copyright_path)?;
    let mut lines = content.split_inclusive(|&b| b == b'\n').peekable();

    // Check the first line for the expected header
    let first_line = match lines.peek() {
        Some(&line) => line,
        None => return Err(FixerError::NoChanges),
    };

    // Strip whitespace, then trailing slashes (like Python's .rstrip().rstrip(b"/"))
    let mut trimmed = first_line;
    while trimmed.last().map_or(false, |&b| is_whitespace(b)) {
        trimmed = &trimmed[..trimmed.len() - 1];
    }
    while trimmed.last() == Some(&b'/') {
        trimmed = &trimmed[..trimmed.len() - 1];
    }

    if trimmed != EXPECTED_HEADER {
        return Err(FixerError::NoChanges);
    }

    let mut new_lines = Vec::new();
    let mut tabs_replaced = false;
    let mut unicode_linebreaks_replaced = false;
    let mut prev_value_offset: Option<usize> = None;

    for line in lines {
        let mut line = line.to_vec();

        // Handle tabs at the start of continuation lines
        if line.starts_with(b"\t") {
            // Try different replacement options to maintain alignment
            let make_option = |prefix: &[u8], skip: usize| {
                let mut v = prefix.to_vec();
                if line.len() > skip {
                    v.extend_from_slice(&line[skip..]);
                }
                v
            };

            let options = [
                make_option(&[b' ', b'\t'], 1),
                make_option(&[b' ', b'\t'], 2),
                make_option(&[b' '; 8], 1),
            ];

            line = options
                .into_iter()
                .find(|opt| value_offset(opt) == prev_value_offset)
                .unwrap_or_else(|| make_option(&[b' ', b'\t'], 1));

            tabs_replaced = true;
        }

        // Handle unicode paragraph separator (replace with two line breaks)
        if line
            .windows(UNICODE_PARAGRAPH_SEPARATOR.len())
            .any(|w| w == UNICODE_PARAGRAPH_SEPARATOR)
        {
            let parts = split_bytes(&line, UNICODE_PARAGRAPH_SEPARATOR);
            let separator = [UNICODE_LINE_BREAK, UNICODE_LINE_BREAK].concat();
            line = join_bytes(parts, &separator);
        }

        // Handle unicode line breaks
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
                        [&[b' '], content].concat()
                    }
                })
                .collect();

            line = join_bytes(new_parts, b"\n");
        }

        prev_value_offset = value_offset(&line);
        new_lines.push(line);
    }

    if !tabs_replaced && !unicode_linebreaks_replaced {
        return Err(FixerError::NoChanges);
    }

    let output: Vec<u8> = new_lines.into_iter().flatten().collect();
    fs::write(&copyright_path, output)?;

    let mut description = "debian/copyright: ".to_string();
    if tabs_replaced {
        description.push_str("use spaces rather than tabs to start continuation lines");
        if unicode_linebreaks_replaced {
            description.push_str(", ");
        }
    }
    if unicode_linebreaks_replaced {
        description.push_str("replace unicode linebreaks with regular linebreaks");
    }
    description.push('.');

    let mut result = FixerResult::builder(description);
    if tabs_replaced {
        result = result.fixed_tag("tab-in-license-text");
    }
    Ok(result.build())
}

declare_fixer! {
    name: "copyright-continued-lines-with-space",
    tags: ["tab-in-license-text"],
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
    fn test_replace_tabs() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        fs::write(
            &copyright_path,
            b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
License: GPL-3+\n\
\tThis is a continuation line\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert!(result
            .description
            .contains("use spaces rather than tabs to start continuation lines"));

        let content = fs::read(&copyright_path).unwrap();
        let expected = b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nLicense: GPL-3+\n \tThis is a continuation line\n";
        assert_eq!(content, expected);
    }

    #[test]
    fn test_no_changes() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        fs::write(
            &copyright_path,
            b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
License: GPL-3+\n\
 This is a continuation line\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_not_machine_readable() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        fs::write(
            &copyright_path,
            b"This is a regular copyright file\n\
Copyright (c) 2024 Someone\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_unicode_linebreaks() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        let mut content = b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nLicense: GPL-3+\n Line one".to_vec();
        content.extend_from_slice(UNICODE_LINE_BREAK);
        content.extend_from_slice(b"Line two\n");
        fs::write(&copyright_path, &content).unwrap();

        let result = run(base_path).unwrap();
        assert!(result
            .description
            .contains("replace unicode linebreaks with regular linebreaks"));

        let new_content = fs::read(&copyright_path).unwrap();
        let expected = b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nLicense: GPL-3+\n Line one\n Line two\n";
        assert_eq!(new_content, expected);
    }

    #[test]
    fn test_no_copyright_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
