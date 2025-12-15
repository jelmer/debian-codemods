//! Parser for INI/.desktop style files.
//!
//! This parser can be used to parse files in the INI/.desktop format (as specified
//! by the [freedesktop.org Desktop Entry Specification](https://specifications.freedesktop.org/desktop-entry-spec/latest/)),
//! while preserving all whitespace and comments. It is based on
//! the [rowan] library, which is a lossless parser library for Rust.
//!
//! Once parsed, the file can be traversed or modified, and then written back to a file.
//!
//! # Example
//!
//! ```
//! use desktop_edit::Desktop;
//! use std::str::FromStr;
//!
//! # let input = r#"[Desktop Entry]
//! # Name=Example Application
//! # Type=Application
//! # Exec=example
//! # Icon=example.png
//! # "#;
//! # let desktop = Desktop::from_str(input).unwrap();
//! # assert_eq!(desktop.groups().count(), 1);
//! # let group = desktop.groups().nth(0).unwrap();
//! # assert_eq!(group.name(), Some("Desktop Entry".to_string()));
//! ```

use crate::lex::{lex, SyntaxKind};
use rowan::ast::AstNode;
use rowan::{GreenNode, GreenNodeBuilder};
use std::path::Path;
use std::str::FromStr;

/// A positioned parse error containing location information.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PositionedParseError {
    /// The error message
    pub message: String,
    /// The text range where the error occurred
    pub range: rowan::TextRange,
    /// Optional error code for categorization
    pub code: Option<String>,
}

impl std::fmt::Display for PositionedParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PositionedParseError {}

/// List of encountered syntax errors.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParseError(pub Vec<String>);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for err in &self.0 {
            writeln!(f, "{}", err)?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}

/// Error parsing INI/.desktop files
#[derive(Debug)]
pub enum Error {
    /// A syntax error was encountered while parsing the file.
    ParseError(ParseError),

    /// An I/O error was encountered while reading the file.
    IoError(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self {
            Error::ParseError(err) => write!(f, "{}", err),
            Error::IoError(err) => write!(f, "{}", err),
        }
    }
}

impl From<ParseError> for Error {
    fn from(err: ParseError) -> Self {
        Self::ParseError(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

impl std::error::Error for Error {}

/// Language definition for rowan
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {}

impl rowan::Language for Lang {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

/// Internal parse result
pub(crate) struct Parse {
    pub(crate) green_node: GreenNode,
    pub(crate) errors: Vec<String>,
    pub(crate) positioned_errors: Vec<PositionedParseError>,
}

/// Parse an INI/.desktop file
pub(crate) fn parse(text: &str) -> Parse {
    struct Parser<'a> {
        tokens: Vec<(SyntaxKind, &'a str)>,
        builder: GreenNodeBuilder<'static>,
        errors: Vec<String>,
        positioned_errors: Vec<PositionedParseError>,
        pos: usize,
    }

    impl<'a> Parser<'a> {
        fn current(&self) -> Option<SyntaxKind> {
            if self.pos < self.tokens.len() {
                Some(self.tokens[self.tokens.len() - 1 - self.pos].0)
            } else {
                None
            }
        }

        fn bump(&mut self) {
            if self.pos < self.tokens.len() {
                let (kind, text) = self.tokens[self.tokens.len() - 1 - self.pos];
                self.builder.token(kind.into(), text);
                self.pos += 1;
            }
        }

        fn skip_ws(&mut self) {
            while self.current() == Some(SyntaxKind::WHITESPACE) {
                self.bump();
            }
        }

        fn skip_blank_lines(&mut self) {
            while let Some(kind) = self.current() {
                match kind {
                    SyntaxKind::NEWLINE => {
                        self.builder.start_node(SyntaxKind::BLANK_LINE.into());
                        self.bump();
                        self.builder.finish_node();
                    }
                    SyntaxKind::WHITESPACE => {
                        // Check if followed by newline
                        if self.pos + 1 < self.tokens.len()
                            && self.tokens[self.tokens.len() - 2 - self.pos].0
                                == SyntaxKind::NEWLINE
                        {
                            self.builder.start_node(SyntaxKind::BLANK_LINE.into());
                            self.bump(); // whitespace
                            self.bump(); // newline
                            self.builder.finish_node();
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        }

        fn parse_group_header(&mut self) {
            self.builder.start_node(SyntaxKind::GROUP_HEADER.into());

            // Consume '['
            if self.current() == Some(SyntaxKind::LEFT_BRACKET) {
                self.bump();
            } else {
                self.errors
                    .push("expected '[' at start of group header".to_string());
            }

            // Consume section name (stored as VALUE tokens)
            if self.current() == Some(SyntaxKind::VALUE) {
                self.bump();
            } else {
                self.errors
                    .push("expected section name in group header".to_string());
            }

            // Consume ']'
            if self.current() == Some(SyntaxKind::RIGHT_BRACKET) {
                self.bump();
            } else {
                self.errors
                    .push("expected ']' at end of group header".to_string());
            }

            // Consume newline if present
            if self.current() == Some(SyntaxKind::NEWLINE) {
                self.bump();
            }

            self.builder.finish_node();
        }

        fn parse_entry(&mut self) {
            self.builder.start_node(SyntaxKind::ENTRY.into());

            // Handle comment before entry
            if self.current() == Some(SyntaxKind::COMMENT) {
                self.bump();
                if self.current() == Some(SyntaxKind::NEWLINE) {
                    self.bump();
                }
                self.builder.finish_node();
                return;
            }

            // Parse key
            if self.current() == Some(SyntaxKind::KEY) {
                self.bump();
            } else {
                self.errors
                    .push(format!("expected key, got {:?}", self.current()));
            }

            self.skip_ws();

            // Check for locale suffix [locale] - note that after KEY, we might get LEFT_BRACKET directly
            // but the lexer treats [ as in_section_header mode, so we need to handle this differently
            // Actually, we need to look for [ character in a key-value context
            // For now, let's check if we have LEFT_BRACKET and handle it as locale
            if self.current() == Some(SyntaxKind::LEFT_BRACKET) {
                self.bump();
                // After [, we should have the locale as VALUE (since lexer is in section header mode)
                // But we need to handle this edge case
                self.skip_ws();
                if self.current() == Some(SyntaxKind::VALUE) {
                    self.bump();
                }
                if self.current() == Some(SyntaxKind::RIGHT_BRACKET) {
                    self.bump();
                }
                self.skip_ws();
            }

            // Parse '='
            if self.current() == Some(SyntaxKind::EQUALS) {
                self.bump();
            } else {
                self.errors.push("expected '=' after key".to_string());
            }

            self.skip_ws();

            // Parse value
            if self.current() == Some(SyntaxKind::VALUE) {
                self.bump();
            }

            // Consume newline if present
            if self.current() == Some(SyntaxKind::NEWLINE) {
                self.bump();
            }

            self.builder.finish_node();
        }

        fn parse_group(&mut self) {
            self.builder.start_node(SyntaxKind::GROUP.into());

            // Parse group header
            self.parse_group_header();

            // Parse entries until we hit another group header or EOF
            while let Some(kind) = self.current() {
                match kind {
                    SyntaxKind::LEFT_BRACKET => break, // Start of next group
                    SyntaxKind::KEY | SyntaxKind::COMMENT => self.parse_entry(),
                    SyntaxKind::NEWLINE | SyntaxKind::WHITESPACE => {
                        self.skip_blank_lines();
                    }
                    _ => {
                        self.errors
                            .push(format!("unexpected token in group: {:?}", kind));
                        self.bump();
                    }
                }
            }

            self.builder.finish_node();
        }

        fn parse_file(&mut self) {
            self.builder.start_node(SyntaxKind::ROOT.into());

            // Skip leading blank lines and comments
            while let Some(kind) = self.current() {
                match kind {
                    SyntaxKind::COMMENT => {
                        self.builder.start_node(SyntaxKind::ENTRY.into());
                        self.bump();
                        if self.current() == Some(SyntaxKind::NEWLINE) {
                            self.bump();
                        }
                        self.builder.finish_node();
                    }
                    SyntaxKind::NEWLINE | SyntaxKind::WHITESPACE => {
                        self.skip_blank_lines();
                    }
                    _ => break,
                }
            }

            // Parse groups
            while self.current().is_some() {
                if self.current() == Some(SyntaxKind::LEFT_BRACKET) {
                    self.parse_group();
                } else {
                    self.errors
                        .push(format!("expected group header, got {:?}", self.current()));
                    self.bump();
                }
            }

            self.builder.finish_node();
        }
    }

    let mut tokens: Vec<_> = lex(text).collect();
    tokens.reverse();

    let mut parser = Parser {
        tokens,
        builder: GreenNodeBuilder::new(),
        errors: Vec::new(),
        positioned_errors: Vec::new(),
        pos: 0,
    };

    parser.parse_file();

    Parse {
        green_node: parser.builder.finish(),
        errors: parser.errors,
        positioned_errors: parser.positioned_errors,
    }
}

// Type aliases for convenience
type SyntaxNode = rowan::SyntaxNode<Lang>;

/// Calculate line and column (both 0-indexed) for the given offset in the tree.
/// Column is measured in bytes from the start of the line.
fn line_col_at_offset(node: &SyntaxNode, offset: rowan::TextSize) -> (usize, usize) {
    let root = node.ancestors().last().unwrap_or_else(|| node.clone());
    let mut line = 0;
    let mut last_newline_offset = rowan::TextSize::from(0);

    for element in root.preorder_with_tokens() {
        if let rowan::WalkEvent::Enter(rowan::NodeOrToken::Token(token)) = element {
            if token.text_range().start() >= offset {
                break;
            }

            // Count newlines and track position of last one
            for (idx, _) in token.text().match_indices('\n') {
                line += 1;
                last_newline_offset =
                    token.text_range().start() + rowan::TextSize::from((idx + 1) as u32);
            }
        }
    }

    let column: usize = (offset - last_newline_offset).into();
    (line, column)
}

/// The root of an INI/.desktop file
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Desktop(SyntaxNode);

impl Desktop {
    /// Get all groups in the file
    pub fn groups(&self) -> impl Iterator<Item = Group> {
        self.0.children().filter_map(Group::cast)
    }

    /// Get a specific group by name
    pub fn get_group(&self, name: &str) -> Option<Group> {
        self.groups().find(|g| g.name().as_deref() == Some(name))
    }

    /// Get the raw syntax node
    pub fn syntax(&self) -> &SyntaxNode {
        &self.0
    }

    /// Convert to a string (same as Display::fmt)
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }

    /// Load from a file
    pub fn from_file(path: &Path) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path)?;
        Self::from_str(&text)
    }

    /// Get the line number (0-indexed) where this node starts.
    pub fn line(&self) -> usize {
        line_col_at_offset(&self.0, self.0.text_range().start()).0
    }

    /// Get the column number (0-indexed, in bytes) where this node starts.
    pub fn column(&self) -> usize {
        line_col_at_offset(&self.0, self.0.text_range().start()).1
    }

    /// Get both line and column (0-indexed) where this node starts.
    /// Returns (line, column) where column is measured in bytes from the start of the line.
    pub fn line_col(&self) -> (usize, usize) {
        line_col_at_offset(&self.0, self.0.text_range().start())
    }
}

impl AstNode for Desktop {
    type Language = Lang;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::ROOT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::ROOT {
            Some(Desktop(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl FromStr for Desktop {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed = parse(s);
        if !parsed.errors.is_empty() {
            return Err(Error::ParseError(ParseError(parsed.errors)));
        }
        let node = SyntaxNode::new_root_mut(parsed.green_node);
        Ok(Desktop::cast(node).expect("root node should be Desktop"))
    }
}

impl std::fmt::Display for Desktop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.text())
    }
}

/// A group/section in an INI/.desktop file (e.g., [Desktop Entry])
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Group(SyntaxNode);

impl Group {
    /// Get the name of the group
    pub fn name(&self) -> Option<String> {
        let header = self
            .0
            .children()
            .find(|n| n.kind() == SyntaxKind::GROUP_HEADER)?;
        let value = header
            .children_with_tokens()
            .find(|e| e.kind() == SyntaxKind::VALUE)?;
        Some(value.as_token()?.text().to_string())
    }

    /// Get all entries in the group
    pub fn entries(&self) -> impl Iterator<Item = Entry> {
        self.0.children().filter_map(Entry::cast)
    }

    /// Get a specific entry by key
    pub fn get(&self, key: &str) -> Option<String> {
        self.entries()
            .find(|e| e.key().as_deref() == Some(key) && e.locale().is_none())
            .and_then(|e| e.value())
    }

    /// Get a localized value for a key (e.g., get_locale("Name", "de"))
    pub fn get_locale(&self, key: &str, locale: &str) -> Option<String> {
        self.entries()
            .find(|e| e.key().as_deref() == Some(key) && e.locale().as_deref() == Some(locale))
            .and_then(|e| e.value())
    }

    /// Get all locales for a given key
    pub fn get_locales(&self, key: &str) -> Vec<String> {
        self.entries()
            .filter(|e| e.key().as_deref() == Some(key) && e.locale().is_some())
            .filter_map(|e| e.locale())
            .collect()
    }

    /// Get all entries for a key (including localized variants)
    pub fn get_all(&self, key: &str) -> Vec<(Option<String>, String)> {
        self.entries()
            .filter(|e| e.key().as_deref() == Some(key))
            .filter_map(|e| {
                let value = e.value()?;
                Some((e.locale(), value))
            })
            .collect()
    }

    /// Set a value for a key (or add if it doesn't exist)
    pub fn set(&mut self, key: &str, value: &str) {
        let new_entry = Entry::new(key, value);

        // Check if the field already exists and replace it
        for entry in self.entries() {
            if entry.key().as_deref() == Some(key) && entry.locale().is_none() {
                self.0.splice_children(
                    entry.0.index()..entry.0.index() + 1,
                    vec![new_entry.0.into()],
                );
                return;
            }
        }

        // Field doesn't exist, append at the end (before the closing of the group)
        let insertion_index = self.0.children_with_tokens().count();
        self.0
            .splice_children(insertion_index..insertion_index, vec![new_entry.0.into()]);
    }

    /// Set a localized value for a key (e.g., set_locale("Name", "de", "Beispiel"))
    pub fn set_locale(&mut self, key: &str, locale: &str, value: &str) {
        let new_entry = Entry::new_localized(key, locale, value);

        // Check if the field already exists and replace it
        for entry in self.entries() {
            if entry.key().as_deref() == Some(key) && entry.locale().as_deref() == Some(locale) {
                self.0.splice_children(
                    entry.0.index()..entry.0.index() + 1,
                    vec![new_entry.0.into()],
                );
                return;
            }
        }

        // Field doesn't exist, append at the end (before the closing of the group)
        let insertion_index = self.0.children_with_tokens().count();
        self.0
            .splice_children(insertion_index..insertion_index, vec![new_entry.0.into()]);
    }

    /// Remove an entry by key (non-localized only)
    pub fn remove(&mut self, key: &str) {
        // Find and remove the entry with the matching key (non-localized)
        let entry_to_remove = self.0.children().find_map(|child| {
            let entry = Entry::cast(child)?;
            if entry.key().as_deref() == Some(key) && entry.locale().is_none() {
                Some(entry)
            } else {
                None
            }
        });

        if let Some(entry) = entry_to_remove {
            entry.syntax().detach();
        }
    }

    /// Remove a localized entry by key and locale
    pub fn remove_locale(&mut self, key: &str, locale: &str) {
        // Find and remove the entry with the matching key and locale
        let entry_to_remove = self.0.children().find_map(|child| {
            let entry = Entry::cast(child)?;
            if entry.key().as_deref() == Some(key) && entry.locale().as_deref() == Some(locale) {
                Some(entry)
            } else {
                None
            }
        });

        if let Some(entry) = entry_to_remove {
            entry.syntax().detach();
        }
    }

    /// Remove all entries for a key (including all localized variants)
    pub fn remove_all(&mut self, key: &str) {
        // Collect all entries to remove first (can't mutate while iterating)
        let entries_to_remove: Vec<_> = self
            .0
            .children()
            .filter_map(Entry::cast)
            .filter(|e| e.key().as_deref() == Some(key))
            .collect();

        for entry in entries_to_remove {
            entry.syntax().detach();
        }
    }

    /// Get the raw syntax node
    pub fn syntax(&self) -> &SyntaxNode {
        &self.0
    }

    /// Get the line number (0-indexed) where this node starts.
    pub fn line(&self) -> usize {
        line_col_at_offset(&self.0, self.0.text_range().start()).0
    }

    /// Get the column number (0-indexed, in bytes) where this node starts.
    pub fn column(&self) -> usize {
        line_col_at_offset(&self.0, self.0.text_range().start()).1
    }

    /// Get both line and column (0-indexed) where this node starts.
    /// Returns (line, column) where column is measured in bytes from the start of the line.
    pub fn line_col(&self) -> (usize, usize) {
        line_col_at_offset(&self.0, self.0.text_range().start())
    }
}

impl AstNode for Group {
    type Language = Lang;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::GROUP
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::GROUP {
            Some(Group(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// A key-value entry in a group
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Entry(SyntaxNode);

impl Entry {
    /// Create a new entry with key=value
    pub fn new(key: &str, value: &str) -> Entry {
        use rowan::GreenNodeBuilder;

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::ENTRY.into());
        builder.token(SyntaxKind::KEY.into(), key);
        builder.token(SyntaxKind::EQUALS.into(), "=");
        builder.token(SyntaxKind::VALUE.into(), value);
        builder.token(SyntaxKind::NEWLINE.into(), "\n");
        builder.finish_node();
        Entry(SyntaxNode::new_root_mut(builder.finish()))
    }

    /// Create a new localized entry with key[locale]=value
    pub fn new_localized(key: &str, locale: &str, value: &str) -> Entry {
        use rowan::GreenNodeBuilder;

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::ENTRY.into());
        builder.token(SyntaxKind::KEY.into(), key);
        builder.token(SyntaxKind::LEFT_BRACKET.into(), "[");
        builder.token(SyntaxKind::VALUE.into(), locale);
        builder.token(SyntaxKind::RIGHT_BRACKET.into(), "]");
        builder.token(SyntaxKind::EQUALS.into(), "=");
        builder.token(SyntaxKind::VALUE.into(), value);
        builder.token(SyntaxKind::NEWLINE.into(), "\n");
        builder.finish_node();
        Entry(SyntaxNode::new_root_mut(builder.finish()))
    }

    /// Get the key name
    pub fn key(&self) -> Option<String> {
        let key_token = self
            .0
            .children_with_tokens()
            .find(|e| e.kind() == SyntaxKind::KEY)?;
        Some(key_token.as_token()?.text().to_string())
    }

    /// Get the value
    pub fn value(&self) -> Option<String> {
        // Find VALUE after EQUALS
        let mut found_equals = false;
        for element in self.0.children_with_tokens() {
            match element.kind() {
                SyntaxKind::EQUALS => found_equals = true,
                SyntaxKind::VALUE if found_equals => {
                    return Some(element.as_token()?.text().to_string());
                }
                _ => {}
            }
        }
        None
    }

    /// Get the locale suffix if present (e.g., "de_DE" from "Name[de_DE]")
    pub fn locale(&self) -> Option<String> {
        // Find VALUE between [ and ] after KEY
        let mut found_key = false;
        let mut in_locale = false;
        for element in self.0.children_with_tokens() {
            match element.kind() {
                SyntaxKind::KEY => found_key = true,
                SyntaxKind::LEFT_BRACKET if found_key && !in_locale => in_locale = true,
                SyntaxKind::VALUE if in_locale => {
                    return Some(element.as_token()?.text().to_string());
                }
                SyntaxKind::RIGHT_BRACKET if in_locale => in_locale = false,
                SyntaxKind::EQUALS => break, // Stop if we reach equals without finding locale
                _ => {}
            }
        }
        None
    }

    /// Get the raw syntax node
    pub fn syntax(&self) -> &SyntaxNode {
        &self.0
    }

    /// Get the line number (0-indexed) where this node starts.
    pub fn line(&self) -> usize {
        line_col_at_offset(&self.0, self.0.text_range().start()).0
    }

    /// Get the column number (0-indexed, in bytes) where this node starts.
    pub fn column(&self) -> usize {
        line_col_at_offset(&self.0, self.0.text_range().start()).1
    }

    /// Get both line and column (0-indexed) where this node starts.
    /// Returns (line, column) where column is measured in bytes from the start of the line.
    pub fn line_col(&self) -> (usize, usize) {
        line_col_at_offset(&self.0, self.0.text_range().start())
    }
}

impl AstNode for Entry {
    type Language = Lang;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::ENTRY
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::ENTRY {
            Some(Entry(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let input = r#"[Desktop Entry]
Name=Example
Type=Application
"#;
        let desktop = Desktop::from_str(input).unwrap();
        assert_eq!(desktop.groups().count(), 1);

        let group = desktop.groups().nth(0).unwrap();
        assert_eq!(group.name(), Some("Desktop Entry".to_string()));
        assert_eq!(group.get("Name"), Some("Example".to_string()));
        assert_eq!(group.get("Type"), Some("Application".to_string()));
    }

    #[test]
    fn test_parse_with_comments() {
        let input = r#"# Top comment
[Desktop Entry]
# Comment before name
Name=Example
Type=Application
"#;
        let desktop = Desktop::from_str(input).unwrap();
        assert_eq!(desktop.groups().count(), 1);

        let group = desktop.groups().nth(0).unwrap();
        assert_eq!(group.get("Name"), Some("Example".to_string()));
    }

    #[test]
    fn test_parse_multiple_groups() {
        let input = r#"[Desktop Entry]
Name=Example

[Desktop Action Play]
Name=Play
Exec=example --play
"#;
        let desktop = Desktop::from_str(input).unwrap();
        assert_eq!(desktop.groups().count(), 2);

        let group1 = desktop.groups().nth(0).unwrap();
        assert_eq!(group1.name(), Some("Desktop Entry".to_string()));

        let group2 = desktop.groups().nth(1).unwrap();
        assert_eq!(group2.name(), Some("Desktop Action Play".to_string()));
        assert_eq!(group2.get("Name"), Some("Play".to_string()));
    }

    #[test]
    fn test_parse_with_spaces() {
        let input = "[Desktop Entry]\nName = Example Application\n";
        let desktop = Desktop::from_str(input).unwrap();

        let group = desktop.groups().nth(0).unwrap();
        assert_eq!(group.get("Name"), Some("Example Application".to_string()));
    }

    #[test]
    fn test_entry_locale() {
        let input = "[Desktop Entry]\nName[de]=Beispiel\n";
        let desktop = Desktop::from_str(input).unwrap();

        let group = desktop.groups().nth(0).unwrap();
        let entry = group.entries().nth(0).unwrap();
        assert_eq!(entry.key(), Some("Name".to_string()));
        assert_eq!(entry.locale(), Some("de".to_string()));
        assert_eq!(entry.value(), Some("Beispiel".to_string()));
    }

    #[test]
    fn test_lossless_roundtrip() {
        let input = r#"# Comment
[Desktop Entry]
Name=Example
Type=Application

[Another Section]
Key=Value
"#;
        let desktop = Desktop::from_str(input).unwrap();
        let output = desktop.text();
        assert_eq!(input, output);
    }

    #[test]
    fn test_localized_query() {
        let input = r#"[Desktop Entry]
Name=Example Application
Name[de]=Beispielanwendung
Name[fr]=Application exemple
Type=Application
"#;
        let desktop = Desktop::from_str(input).unwrap();
        let group = desktop.groups().nth(0).unwrap();

        // Test get() returns non-localized value
        assert_eq!(group.get("Name"), Some("Example Application".to_string()));

        // Test get_locale() returns localized values
        assert_eq!(
            group.get_locale("Name", "de"),
            Some("Beispielanwendung".to_string())
        );
        assert_eq!(
            group.get_locale("Name", "fr"),
            Some("Application exemple".to_string())
        );
        assert_eq!(group.get_locale("Name", "es"), None);

        // Test get_locales() returns all locales for a key
        let locales = group.get_locales("Name");
        assert_eq!(locales.len(), 2);
        assert!(locales.contains(&"de".to_string()));
        assert!(locales.contains(&"fr".to_string()));

        // Test get_all() returns all variants
        let all = group.get_all("Name");
        assert_eq!(all.len(), 3);
        assert!(all.contains(&(None, "Example Application".to_string())));
        assert!(all.contains(&(Some("de".to_string()), "Beispielanwendung".to_string())));
        assert!(all.contains(&(Some("fr".to_string()), "Application exemple".to_string())));
    }

    #[test]
    fn test_localized_set() {
        let input = r#"[Desktop Entry]
Name=Example
Name[de]=Beispiel
Type=Application
"#;
        let desktop = Desktop::from_str(input).unwrap();
        {
            let mut group = desktop.groups().nth(0).unwrap();
            // Update localized value
            group.set_locale("Name", "de", "Neue Beispiel");
        }

        // Re-fetch the group to check the mutation persisted
        let group = desktop.groups().nth(0).unwrap();
        assert_eq!(
            group.get_locale("Name", "de"),
            Some("Neue Beispiel".to_string())
        );

        // Original value should remain unchanged
        assert_eq!(group.get("Name"), Some("Example".to_string()));
    }

    #[test]
    fn test_localized_remove() {
        let input = r#"[Desktop Entry]
Name=Example
Name[de]=Beispiel
Name[fr]=Exemple
Type=Application
"#;
        let desktop = Desktop::from_str(input).unwrap();
        let mut group = desktop.groups().nth(0).unwrap();

        // Remove one localized entry
        group.remove_locale("Name", "de");
        assert_eq!(group.get_locale("Name", "de"), None);
        assert_eq!(group.get_locale("Name", "fr"), Some("Exemple".to_string()));
        assert_eq!(group.get("Name"), Some("Example".to_string()));

        // Remove non-localized entry
        group.remove("Name");
        assert_eq!(group.get("Name"), None);
        assert_eq!(group.get_locale("Name", "fr"), Some("Exemple".to_string()));
    }

    #[test]
    fn test_localized_remove_all() {
        let input = r#"[Desktop Entry]
Name=Example
Name[de]=Beispiel
Name[fr]=Exemple
Type=Application
"#;
        let desktop = Desktop::from_str(input).unwrap();
        let mut group = desktop.groups().nth(0).unwrap();

        // Remove all Name entries
        group.remove_all("Name");
        assert_eq!(group.get("Name"), None);
        assert_eq!(group.get_locale("Name", "de"), None);
        assert_eq!(group.get_locale("Name", "fr"), None);
        assert_eq!(group.get_locales("Name").len(), 0);

        // Type should still be there
        assert_eq!(group.get("Type"), Some("Application".to_string()));
    }

    #[test]
    fn test_get_distinguishes_localized() {
        let input = r#"[Desktop Entry]
Name[de]=Beispiel
Type=Application
"#;
        let desktop = Desktop::from_str(input).unwrap();
        let group = desktop.groups().nth(0).unwrap();

        // get() should not return localized entries
        assert_eq!(group.get("Name"), None);
        assert_eq!(group.get_locale("Name", "de"), Some("Beispiel".to_string()));
    }

    #[test]
    fn test_add_new_entry() {
        let input = r#"[Desktop Entry]
Name=Example
"#;
        let desktop = Desktop::from_str(input).unwrap();
        {
            let mut group = desktop.groups().nth(0).unwrap();
            // Add a new entry
            group.set("Type", "Application");
        }

        let group = desktop.groups().nth(0).unwrap();
        assert_eq!(group.get("Name"), Some("Example".to_string()));
        assert_eq!(group.get("Type"), Some("Application".to_string()));
    }

    #[test]
    fn test_add_new_localized_entry() {
        let input = r#"[Desktop Entry]
Name=Example
"#;
        let desktop = Desktop::from_str(input).unwrap();
        {
            let mut group = desktop.groups().nth(0).unwrap();
            // Add new localized entries
            group.set_locale("Name", "de", "Beispiel");
            group.set_locale("Name", "fr", "Exemple");
        }

        let group = desktop.groups().nth(0).unwrap();
        assert_eq!(group.get("Name"), Some("Example".to_string()));
        assert_eq!(group.get_locale("Name", "de"), Some("Beispiel".to_string()));
        assert_eq!(group.get_locale("Name", "fr"), Some("Exemple".to_string()));
        assert_eq!(group.get_locales("Name").len(), 2);
    }

    #[test]
    fn test_line_col() {
        let text = r#"[Desktop Entry]
Name=Example Application
Type=Application
Exec=example

[Desktop Action Play]
Name=Play
Exec=example --play
"#;
        let desktop = Desktop::from_str(text).unwrap();

        // Test desktop root starts at line 0
        assert_eq!(desktop.line(), 0);
        assert_eq!(desktop.column(), 0);

        // Test group line numbers
        let groups: Vec<_> = desktop.groups().collect();
        assert_eq!(groups.len(), 2);

        // First group starts at line 0
        assert_eq!(groups[0].line(), 0);
        assert_eq!(groups[0].column(), 0);

        // Second group starts at line 5 (after empty line)
        assert_eq!(groups[1].line(), 5);
        assert_eq!(groups[1].column(), 0);

        // Test entry line numbers in first group
        let entries: Vec<_> = groups[0].entries().collect();
        assert_eq!(entries[0].line(), 1); // Name=Example Application
        assert_eq!(entries[1].line(), 2); // Type=Application
        assert_eq!(entries[2].line(), 3); // Exec=example

        // Test column numbers
        assert_eq!(entries[0].column(), 0); // Start of line
        assert_eq!(entries[1].column(), 0); // Start of line

        // Test line_col() method
        assert_eq!(groups[1].line_col(), (5, 0));
        assert_eq!(entries[0].line_col(), (1, 0));

        // Test entries in second group
        let second_group_entries: Vec<_> = groups[1].entries().collect();
        assert_eq!(second_group_entries[0].line(), 6); // Name=Play
        assert_eq!(second_group_entries[1].line(), 7); // Exec=example --play
    }
}
