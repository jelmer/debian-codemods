//! Lexer for systemd unit files

/// Token types for systemd unit files
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(non_camel_case_types)]
#[repr(u16)]
pub enum SyntaxKind {
    /// Left bracket: `[`
    LEFT_BRACKET = 0,
    /// Right bracket: `]`
    RIGHT_BRACKET,
    /// Equals sign: `=`
    EQUALS,
    /// Key name (e.g., "Type", "ExecStart")
    KEY,
    /// Section name (e.g., "Unit", "Service")
    SECTION_NAME,
    /// Value part of key=value
    VALUE,
    /// Comment starting with `#` or `;`
    COMMENT,
    /// Newline: `\n` or `\r\n`
    NEWLINE,
    /// Whitespace: spaces and tabs
    WHITESPACE,
    /// Line continuation: backslash at end of line
    LINE_CONTINUATION,
    /// Error token
    ERROR,

    /// Root node: the entire file
    ROOT,
    /// Section node: a section with its entries
    SECTION,
    /// Section header node: `[Section Name]`
    SECTION_HEADER,
    /// Entry node: `Key=Value`
    ENTRY,
    /// Blank line node
    BLANK_LINE,
}

/// Convert our `SyntaxKind` into the rowan `SyntaxKind`.
impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

/// Check if a character is valid at the start of a key name
#[inline]
fn is_valid_initial_key_char(c: char) -> bool {
    // Keys must start with A-Za-z
    c.is_ascii_alphabetic()
}

/// Check if a character is valid in a key name
#[inline]
fn is_valid_key_char(c: char) -> bool {
    // Keys can contain A-Za-z0-9_-
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Check if a character is a newline
#[inline]
fn is_newline(c: char) -> bool {
    c == '\n' || c == '\r'
}

/// Check if a character is whitespace (space or tab)
#[inline]
fn is_whitespace(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Lexer implementation
fn lex_impl(input: &str) -> impl Iterator<Item = (SyntaxKind, &str)> + '_ {
    let mut remaining = input;
    let mut at_line_start = true;
    let mut in_section_header = false;

    std::iter::from_fn(move || {
        if remaining.is_empty() {
            return None;
        }

        let c = remaining.chars().next()?;

        match c {
            // Newline
            _ if is_newline(c) => {
                let char_len = c.len_utf8();
                // Handle \r\n as a single newline
                if c == '\r' && remaining.get(1..2) == Some("\n") {
                    let (token, rest) = remaining.split_at(2);
                    remaining = rest;
                    at_line_start = true;
                    in_section_header = false;
                    Some((SyntaxKind::NEWLINE, token))
                } else {
                    let (token, rest) = remaining.split_at(char_len);
                    remaining = rest;
                    at_line_start = true;
                    in_section_header = false;
                    Some((SyntaxKind::NEWLINE, token))
                }
            }

            // Comment (# or ; at start of line or after whitespace)
            '#' | ';' if at_line_start => {
                let end = remaining.find(is_newline).unwrap_or(remaining.len());
                let (token, rest) = remaining.split_at(end);
                remaining = rest;
                Some((SyntaxKind::COMMENT, token))
            }

            // Line continuation (backslash before newline)
            '\\' if remaining.get(1..2) == Some("\n") || remaining.get(1..3) == Some("\r\n") => {
                let len = if remaining.get(1..3) == Some("\r\n") {
                    3
                } else {
                    2
                };
                let (token, rest) = remaining.split_at(len);
                remaining = rest;
                at_line_start = false; // Line continues, so we're not at the start of a new logical line
                Some((SyntaxKind::LINE_CONTINUATION, token))
            }

            // Section header [Section Name]
            '[' if at_line_start => {
                remaining = &remaining[1..]; // consume '['
                at_line_start = false;
                in_section_header = true;
                Some((SyntaxKind::LEFT_BRACKET, "["))
            }

            ']' if in_section_header => {
                remaining = &remaining[1..]; // consume ']'
                in_section_header = false;
                Some((SyntaxKind::RIGHT_BRACKET, "]"))
            }

            // Whitespace at start of line - could be blank line
            _ if is_whitespace(c) && at_line_start => {
                let end = remaining
                    .find(|c| !is_whitespace(c))
                    .unwrap_or(remaining.len());
                let (token, rest) = remaining.split_at(end);
                remaining = rest;
                Some((SyntaxKind::WHITESPACE, token))
            }

            // Whitespace (not at line start)
            _ if is_whitespace(c) => {
                let end = remaining
                    .find(|c| !is_whitespace(c))
                    .unwrap_or(remaining.len());
                let (token, rest) = remaining.split_at(end);
                remaining = rest;
                Some((SyntaxKind::WHITESPACE, token))
            }

            // Equals sign
            '=' => {
                remaining = &remaining[1..];
                Some((SyntaxKind::EQUALS, "="))
            }

            // Key name (starts with alphabetic)
            _ if is_valid_initial_key_char(c) && at_line_start => {
                let end = remaining
                    .find(|c: char| !is_valid_key_char(c))
                    .unwrap_or(remaining.len());
                let (token, rest) = remaining.split_at(end);
                remaining = rest;
                at_line_start = false;
                Some((SyntaxKind::KEY, token))
            }

            // Section name (between [ and ])
            _ if in_section_header => {
                // Inside brackets - read until ]
                let end = remaining.find(']').unwrap_or(remaining.len());
                let (token, rest) = remaining.split_at(end);
                remaining = rest;
                Some((SyntaxKind::SECTION_NAME, token))
            }

            // Value (everything else on a line, handling line continuations)
            _ if !at_line_start => {
                // Read until newline (but watch for line continuations)
                let mut end = 0;
                for ch in remaining.chars() {
                    if ch == '\\' {
                        // Check if it's a line continuation
                        let remaining_from_here = &remaining[end..];
                        if remaining_from_here.get(1..2) == Some("\n")
                            || remaining_from_here.get(1..3) == Some("\r\n")
                        {
                            // It's a line continuation, stop here
                            break;
                        }
                        end += ch.len_utf8();
                    } else if is_newline(ch) {
                        // Stop at newline
                        break;
                    } else {
                        end += ch.len_utf8();
                    }
                }

                if end == 0 {
                    // No value content, this shouldn't happen
                    None
                } else {
                    let (token, rest) = remaining.split_at(end);
                    remaining = rest;
                    Some((SyntaxKind::VALUE, token))
                }
            }

            // Error: unexpected character at line start
            _ => {
                let char_len = c.len_utf8();
                let (token, rest) = remaining.split_at(char_len);
                remaining = rest;
                at_line_start = false;
                Some((SyntaxKind::ERROR, token))
            }
        }
    })
}

/// Lex a systemd unit file into tokens
pub(crate) fn lex(input: &str) -> impl Iterator<Item = (SyntaxKind, &str)> {
    lex_impl(input)
}

#[cfg(test)]
mod tests {
    use super::SyntaxKind::*;
    use super::*;

    #[test]
    fn test_empty() {
        assert_eq!(lex("").collect::<Vec<_>>(), vec![]);
    }

    #[test]
    fn test_simple_section() {
        let input = "[Unit]\n";
        assert_eq!(
            lex(input).collect::<Vec<_>>(),
            vec![
                (LEFT_BRACKET, "["),
                (SECTION_NAME, "Unit"),
                (RIGHT_BRACKET, "]"),
                (NEWLINE, "\n"),
            ]
        );
    }

    #[test]
    fn test_key_value() {
        let input = "Description=Test Service\n";
        assert_eq!(
            lex(input).collect::<Vec<_>>(),
            vec![
                (KEY, "Description"),
                (EQUALS, "="),
                (VALUE, "Test Service"),
                (NEWLINE, "\n"),
            ]
        );
    }

    #[test]
    fn test_key_value_with_spaces() {
        let input = "Description = Test Service\n";
        assert_eq!(
            lex(input).collect::<Vec<_>>(),
            vec![
                (KEY, "Description"),
                (WHITESPACE, " "),
                (EQUALS, "="),
                (WHITESPACE, " "),
                (VALUE, "Test Service"),
                (NEWLINE, "\n"),
            ]
        );
    }

    #[test]
    fn test_comment_hash() {
        let input = "# This is a comment\n";
        assert_eq!(
            lex(input).collect::<Vec<_>>(),
            vec![(COMMENT, "# This is a comment"), (NEWLINE, "\n"),]
        );
    }

    #[test]
    fn test_comment_semicolon() {
        let input = "; This is a comment\n";
        assert_eq!(
            lex(input).collect::<Vec<_>>(),
            vec![(COMMENT, "; This is a comment"), (NEWLINE, "\n"),]
        );
    }

    #[test]
    fn test_line_continuation() {
        let input = "ExecStart=/bin/echo \\\n  hello\n";
        let tokens: Vec<_> = lex(input).collect();
        assert_eq!(tokens[0], (KEY, "ExecStart"));
        assert_eq!(tokens[1], (EQUALS, "="));
        assert_eq!(tokens[2], (VALUE, "/bin/echo "));
        assert_eq!(tokens[3], (LINE_CONTINUATION, "\\\n"));
        assert_eq!(tokens[4], (WHITESPACE, "  "));
        assert_eq!(tokens[5], (VALUE, "hello"));
        assert_eq!(tokens[6], (NEWLINE, "\n"));
    }

    #[test]
    fn test_full_unit_file() {
        let input = r#"[Unit]
Description=Test Service
After=network.target

[Service]
Type=simple
ExecStart=/usr/bin/test
"#;
        let tokens: Vec<_> = lex(input).collect();

        // Verify we get the expected token types
        assert_eq!(tokens[0].0, LEFT_BRACKET);
        assert_eq!(tokens[1].0, SECTION_NAME);
        assert_eq!(tokens[1].1, "Unit");
        assert_eq!(tokens[2].0, RIGHT_BRACKET);
        assert_eq!(tokens[3].0, NEWLINE);

        // Find "Description=Test Service"
        let desc_idx = tokens
            .iter()
            .position(|(k, t)| *k == KEY && *t == "Description")
            .unwrap();
        assert_eq!(tokens[desc_idx + 1].0, EQUALS);
        assert_eq!(tokens[desc_idx + 2].0, VALUE);
        assert_eq!(tokens[desc_idx + 2].1, "Test Service");
    }

    #[test]
    fn test_blank_lines() {
        let input = "Key=Value\n\nKey2=Value2\n";
        let tokens: Vec<_> = lex(input).collect();

        // Should have two newlines in sequence
        let first_newline = tokens.iter().position(|(k, _)| *k == NEWLINE).unwrap();
        assert_eq!(tokens[first_newline + 1].0, NEWLINE);
    }
}
