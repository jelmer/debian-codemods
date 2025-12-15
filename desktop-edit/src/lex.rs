//! Lexer for INI/.desktop files

/// Token types for INI/.desktop files
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
    /// Key name (e.g., "Name", "Type")
    KEY,
    /// Section name (e.g., "Desktop Entry")
    SECTION_NAME,
    /// Locale suffix (e.g., "[de_DE]" in "Name[de_DE]")
    LOCALE,
    /// Value part of key=value
    VALUE,
    /// Comment starting with `#`
    COMMENT,
    /// Newline: `\n` or `\r\n`
    NEWLINE,
    /// Whitespace: spaces and tabs
    WHITESPACE,
    /// Error token
    ERROR,

    /// Root node: the entire file
    ROOT,
    /// Group node: a section with its entries
    GROUP,
    /// Group header node: `[Section Name]`
    GROUP_HEADER,
    /// Entry node: `Key=Value` or `Key[locale]=Value`
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
    // Keys must start with A-Za-z0-9
    c.is_ascii_alphanumeric()
}

/// Check if a character is valid in a key name
#[inline]
fn is_valid_key_char(c: char) -> bool {
    // Keys can contain A-Za-z0-9-
    c.is_ascii_alphanumeric() || c == '-'
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
    let mut in_locale = false;

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
                    in_locale = false;
                    Some((SyntaxKind::NEWLINE, token))
                } else {
                    let (token, rest) = remaining.split_at(char_len);
                    remaining = rest;
                    at_line_start = true;
                    in_section_header = false;
                    in_locale = false;
                    Some((SyntaxKind::NEWLINE, token))
                }
            }

            // Comment (# at start of line or after whitespace)
            '#' if at_line_start => {
                let end = remaining.find(is_newline).unwrap_or(remaining.len());
                let (token, rest) = remaining.split_at(end);
                remaining = rest;
                Some((SyntaxKind::COMMENT, token))
            }

            // Section header [Section Name]
            '[' if at_line_start => {
                remaining = &remaining[1..]; // consume '['
                at_line_start = false;
                in_section_header = true;
                Some((SyntaxKind::LEFT_BRACKET, "["))
            }

            // Left bracket in key-value context (for locale like Name[de])
            '[' => {
                remaining = &remaining[1..]; // consume '['
                in_locale = true;
                Some((SyntaxKind::LEFT_BRACKET, "["))
            }

            ']' => {
                remaining = &remaining[1..]; // consume ']'
                in_section_header = false;
                in_locale = false;
                Some((SyntaxKind::RIGHT_BRACKET, "]"))
            }

            // Whitespace at start of line - could be blank line
            _ if is_whitespace(c) && at_line_start => {
                let end = remaining
                    .find(|c| !is_whitespace(c))
                    .unwrap_or(remaining.len());
                let (token, rest) = remaining.split_at(end);
                remaining = rest;
                // Check if this is followed by newline or EOF (blank line)
                // Otherwise it's just leading whitespace before a key
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

            // Key name (starts with alphanumeric)
            _ if is_valid_initial_key_char(c) && at_line_start => {
                let end = remaining
                    .find(|c: char| !is_valid_key_char(c))
                    .unwrap_or(remaining.len());
                let (token, rest) = remaining.split_at(end);
                remaining = rest;
                at_line_start = false;
                Some((SyntaxKind::KEY, token))
            }

            // Locale identifier or section name (between [ and ])
            _ if in_section_header || in_locale => {
                // Inside brackets - read until ]
                let end = remaining.find(']').unwrap_or(remaining.len());
                let (token, rest) = remaining.split_at(end);
                remaining = rest;
                Some((SyntaxKind::VALUE, token))
            }

            // Value (everything else on a line)
            _ if !at_line_start => {
                // Everything else on the line is a value
                let end = remaining.find(is_newline).unwrap_or(remaining.len());
                let (token, rest) = remaining.split_at(end);
                remaining = rest;
                Some((SyntaxKind::VALUE, token))
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

/// Lex an INI/.desktop file into tokens
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
        let input = "[Desktop Entry]\n";
        assert_eq!(
            lex(input).collect::<Vec<_>>(),
            vec![
                (LEFT_BRACKET, "["),
                (VALUE, "Desktop Entry"),
                (RIGHT_BRACKET, "]"),
                (NEWLINE, "\n"),
            ]
        );
    }

    #[test]
    fn test_key_value() {
        let input = "Name=Example\n";
        assert_eq!(
            lex(input).collect::<Vec<_>>(),
            vec![
                (KEY, "Name"),
                (EQUALS, "="),
                (VALUE, "Example"),
                (NEWLINE, "\n"),
            ]
        );
    }

    #[test]
    fn test_key_value_with_spaces() {
        let input = "Name = Example Application\n";
        assert_eq!(
            lex(input).collect::<Vec<_>>(),
            vec![
                (KEY, "Name"),
                (WHITESPACE, " "),
                (EQUALS, "="),
                (WHITESPACE, " "),
                (VALUE, "Example Application"),
                (NEWLINE, "\n"),
            ]
        );
    }

    #[test]
    fn test_comment() {
        let input = "# This is a comment\n";
        assert_eq!(
            lex(input).collect::<Vec<_>>(),
            vec![(COMMENT, "# This is a comment"), (NEWLINE, "\n"),]
        );
    }

    #[test]
    fn test_full_desktop_file() {
        let input = r#"[Desktop Entry]
Name=Example
Type=Application
Exec=example
# Comment
Icon=example.png

[Desktop Action Play]
Name=Play
Exec=example --play
"#;
        let tokens: Vec<_> = lex(input).collect();

        // Verify we get the expected token types
        assert_eq!(tokens[0].0, LEFT_BRACKET);
        assert_eq!(tokens[1].0, VALUE); // "Desktop Entry"
        assert_eq!(tokens[2].0, RIGHT_BRACKET);
        assert_eq!(tokens[3].0, NEWLINE);

        // Find and verify "Name=Example"
        let name_idx = tokens
            .iter()
            .position(|(k, t)| *k == KEY && *t == "Name")
            .unwrap();
        assert_eq!(tokens[name_idx + 1].0, EQUALS);
        assert_eq!(tokens[name_idx + 2].0, VALUE);
        assert_eq!(tokens[name_idx + 2].1, "Example");
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
