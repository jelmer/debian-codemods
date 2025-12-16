//! Parser for systemd unit files.
//!
//! This parser can be used to parse systemd unit files (as specified
//! by the [systemd.syntax(7)](https://www.freedesktop.org/software/systemd/man/latest/systemd.syntax.html)),
//! while preserving all whitespace and comments. It is based on
//! the [rowan] library, which is a lossless parser library for Rust.
//!
//! Once parsed, the file can be traversed or modified, and then written back to a file.
//!
//! # Example
//!
//! ```
//! use systemd_unit_edit::SystemdUnit;
//! use std::str::FromStr;
//!
//! # let input = r#"[Unit]
//! # Description=Test Service
//! # After=network.target
//! #
//! # [Service]
//! # Type=simple
//! # ExecStart=/usr/bin/test
//! # "#;
//! # let unit = SystemdUnit::from_str(input).unwrap();
//! # assert_eq!(unit.sections().count(), 2);
//! # let section = unit.sections().next().unwrap();
//! # assert_eq!(section.name(), Some("Unit".to_string()));
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

/// Error parsing systemd unit files
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
pub(crate) struct ParseResult {
    pub(crate) green_node: GreenNode,
    pub(crate) errors: Vec<String>,
    pub(crate) positioned_errors: Vec<PositionedParseError>,
}

/// Parse a systemd unit file
pub(crate) fn parse(text: &str) -> ParseResult {
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

        fn parse_section_header(&mut self) {
            self.builder.start_node(SyntaxKind::SECTION_HEADER.into());

            // Consume '['
            if self.current() == Some(SyntaxKind::LEFT_BRACKET) {
                self.bump();
            } else {
                self.errors
                    .push("expected '[' at start of section header".to_string());
            }

            // Consume section name
            if self.current() == Some(SyntaxKind::SECTION_NAME) {
                self.bump();
            } else {
                self.errors
                    .push("expected section name in section header".to_string());
            }

            // Consume ']'
            if self.current() == Some(SyntaxKind::RIGHT_BRACKET) {
                self.bump();
            } else {
                self.errors
                    .push("expected ']' at end of section header".to_string());
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

            // Parse '='
            if self.current() == Some(SyntaxKind::EQUALS) {
                self.bump();
            } else {
                self.errors.push("expected '=' after key".to_string());
            }

            self.skip_ws();

            // Parse value (may include line continuations)
            while let Some(kind) = self.current() {
                match kind {
                    SyntaxKind::VALUE => self.bump(),
                    SyntaxKind::LINE_CONTINUATION => {
                        self.bump();
                        // After line continuation, skip leading whitespace
                        self.skip_ws();
                    }
                    SyntaxKind::NEWLINE => {
                        self.bump();
                        break;
                    }
                    _ => break,
                }
            }

            self.builder.finish_node();
        }

        fn parse_section(&mut self) {
            self.builder.start_node(SyntaxKind::SECTION.into());

            // Parse section header
            self.parse_section_header();

            // Parse entries until we hit another section header or EOF
            while let Some(kind) = self.current() {
                match kind {
                    SyntaxKind::LEFT_BRACKET => break, // Start of next section
                    SyntaxKind::KEY | SyntaxKind::COMMENT => self.parse_entry(),
                    SyntaxKind::NEWLINE => {
                        self.skip_blank_lines();
                    }
                    SyntaxKind::WHITESPACE => {
                        // Try to skip blank lines, but if whitespace is not part of a blank line,
                        // consume it as an error to avoid infinite loop
                        let pos_before = self.pos;
                        self.skip_blank_lines();
                        if self.pos == pos_before {
                            // skip_blank_lines didn't consume anything, so this whitespace
                            // is not part of a blank line (e.g., leading whitespace on a line)
                            self.errors.push("unexpected whitespace at start of line (should be indented continuation or blank line)".to_string());
                            self.bump();
                        }
                    }
                    _ => {
                        self.errors
                            .push(format!("unexpected token in section: {:?}", kind));
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

            // Parse sections
            while self.current().is_some() {
                if self.current() == Some(SyntaxKind::LEFT_BRACKET) {
                    self.parse_section();
                } else {
                    self.errors
                        .push(format!("expected section header, got {:?}", self.current()));
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

    ParseResult {
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

/// The root of a systemd unit file
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SystemdUnit(SyntaxNode);

impl SystemdUnit {
    /// Get all sections in the file
    pub fn sections(&self) -> impl Iterator<Item = Section> {
        self.0.children().filter_map(Section::cast)
    }

    /// Get a specific section by name
    pub fn get_section(&self, name: &str) -> Option<Section> {
        self.sections().find(|s| s.name().as_deref() == Some(name))
    }

    /// Add a new section to the unit file
    pub fn add_section(&mut self, name: &str) {
        let new_section = Section::new(name);
        let insertion_index = self.0.children_with_tokens().count();
        self.0
            .splice_children(insertion_index..insertion_index, vec![new_section.0.into()]);
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

    /// Write to a file
    pub fn write_to_file(&self, path: &Path) -> Result<(), Error> {
        std::fs::write(path, self.text())?;
        Ok(())
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

impl AstNode for SystemdUnit {
    type Language = Lang;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::ROOT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::ROOT {
            Some(SystemdUnit(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl FromStr for SystemdUnit {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed = parse(s);
        if !parsed.errors.is_empty() {
            return Err(Error::ParseError(ParseError(parsed.errors)));
        }
        let node = SyntaxNode::new_root_mut(parsed.green_node);
        Ok(SystemdUnit::cast(node).expect("root node should be SystemdUnit"))
    }
}

impl std::fmt::Display for SystemdUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.text())
    }
}

/// A section in a systemd unit file (e.g., [Unit], [Service])
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Section(SyntaxNode);

impl Section {
    /// Create a new section with the given name
    pub fn new(name: &str) -> Section {
        use rowan::GreenNodeBuilder;

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::SECTION.into());

        // Build section header
        builder.start_node(SyntaxKind::SECTION_HEADER.into());
        builder.token(SyntaxKind::LEFT_BRACKET.into(), "[");
        builder.token(SyntaxKind::SECTION_NAME.into(), name);
        builder.token(SyntaxKind::RIGHT_BRACKET.into(), "]");
        builder.token(SyntaxKind::NEWLINE.into(), "\n");
        builder.finish_node();

        builder.finish_node();
        Section(SyntaxNode::new_root_mut(builder.finish()))
    }

    /// Get the name of the section
    pub fn name(&self) -> Option<String> {
        let header = self
            .0
            .children()
            .find(|n| n.kind() == SyntaxKind::SECTION_HEADER)?;
        let value = header
            .children_with_tokens()
            .find(|e| e.kind() == SyntaxKind::SECTION_NAME)?;
        Some(value.as_token()?.text().to_string())
    }

    /// Get all entries in the section
    pub fn entries(&self) -> impl Iterator<Item = Entry> {
        self.0.children().filter_map(Entry::cast)
    }

    /// Get a specific entry by key
    pub fn get(&self, key: &str) -> Option<String> {
        self.entries()
            .find(|e| e.key().as_deref() == Some(key))
            .and_then(|e| e.value())
    }

    /// Get all values for a key (systemd allows multiple entries with the same key)
    pub fn get_all(&self, key: &str) -> Vec<String> {
        self.entries()
            .filter(|e| e.key().as_deref() == Some(key))
            .filter_map(|e| e.value())
            .collect()
    }

    /// Set a value for a key (replaces the first occurrence or adds if it doesn't exist)
    pub fn set(&mut self, key: &str, value: &str) {
        let new_entry = Entry::new(key, value);

        // Check if the field already exists and replace the first occurrence
        for entry in self.entries() {
            if entry.key().as_deref() == Some(key) {
                self.0.splice_children(
                    entry.0.index()..entry.0.index() + 1,
                    vec![new_entry.0.into()],
                );
                return;
            }
        }

        // Field doesn't exist, append at the end (before trailing whitespace)
        let children: Vec<_> = self.0.children_with_tokens().collect();
        let insertion_index = children
            .iter()
            .enumerate()
            .rev()
            .find(|(_, child)| {
                child.kind() != SyntaxKind::BLANK_LINE
                    && child.kind() != SyntaxKind::NEWLINE
                    && child.kind() != SyntaxKind::WHITESPACE
            })
            .map(|(idx, _)| idx + 1)
            .unwrap_or(children.len());

        self.0
            .splice_children(insertion_index..insertion_index, vec![new_entry.0.into()]);
    }

    /// Add a value for a key (appends even if the key already exists)
    pub fn add(&mut self, key: &str, value: &str) {
        let new_entry = Entry::new(key, value);

        // Find the last non-whitespace child to insert after
        let children: Vec<_> = self.0.children_with_tokens().collect();
        let insertion_index = children
            .iter()
            .enumerate()
            .rev()
            .find(|(_, child)| {
                child.kind() != SyntaxKind::BLANK_LINE
                    && child.kind() != SyntaxKind::NEWLINE
                    && child.kind() != SyntaxKind::WHITESPACE
            })
            .map(|(idx, _)| idx + 1)
            .unwrap_or(children.len());

        self.0
            .splice_children(insertion_index..insertion_index, vec![new_entry.0.into()]);
    }

    /// Insert a value at a specific position (index is among entries only, not all nodes)
    ///
    /// If the index is greater than or equal to the number of entries, the entry
    /// will be appended at the end.
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SystemdUnit;
    /// # use std::str::FromStr;
    /// let input = r#"[Unit]
    /// Description=Test Service
    /// After=network.target
    /// "#;
    /// let unit = SystemdUnit::from_str(input).unwrap();
    /// {
    ///     let mut section = unit.get_section("Unit").unwrap();
    ///     section.insert_at(1, "Wants", "foo.service");
    /// }
    ///
    /// let section = unit.get_section("Unit").unwrap();
    /// let entries: Vec<_> = section.entries().collect();
    /// assert_eq!(entries[0].key(), Some("Description".to_string()));
    /// assert_eq!(entries[1].key(), Some("Wants".to_string()));
    /// assert_eq!(entries[2].key(), Some("After".to_string()));
    /// ```
    pub fn insert_at(&mut self, index: usize, key: &str, value: &str) {
        let new_entry = Entry::new(key, value);

        // Find the insertion point by counting entries
        let entries: Vec<_> = self.entries().collect();

        if index >= entries.len() {
            // If index is beyond the end, just append
            self.add(key, value);
        } else {
            // Insert at the specified entry position
            let target_entry = &entries[index];
            let insertion_index = target_entry.0.index();
            self.0
                .splice_children(insertion_index..insertion_index, vec![new_entry.0.into()]);
        }
    }

    /// Insert a value before the first entry with the specified key
    ///
    /// If no entry with the specified key exists, this method does nothing.
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SystemdUnit;
    /// # use std::str::FromStr;
    /// let input = r#"[Unit]
    /// Description=Test Service
    /// After=network.target
    /// "#;
    /// let unit = SystemdUnit::from_str(input).unwrap();
    /// {
    ///     let mut section = unit.get_section("Unit").unwrap();
    ///     section.insert_before("After", "Wants", "foo.service");
    /// }
    ///
    /// let section = unit.get_section("Unit").unwrap();
    /// let entries: Vec<_> = section.entries().collect();
    /// assert_eq!(entries[0].key(), Some("Description".to_string()));
    /// assert_eq!(entries[1].key(), Some("Wants".to_string()));
    /// assert_eq!(entries[2].key(), Some("After".to_string()));
    /// ```
    pub fn insert_before(&mut self, existing_key: &str, key: &str, value: &str) {
        let new_entry = Entry::new(key, value);

        // Find the first entry with the matching key
        let target_entry = self
            .entries()
            .find(|e| e.key().as_deref() == Some(existing_key));

        if let Some(entry) = target_entry {
            let insertion_index = entry.0.index();
            self.0
                .splice_children(insertion_index..insertion_index, vec![new_entry.0.into()]);
        }
        // If the key doesn't exist, do nothing
    }

    /// Insert a value after the first entry with the specified key
    ///
    /// If no entry with the specified key exists, this method does nothing.
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SystemdUnit;
    /// # use std::str::FromStr;
    /// let input = r#"[Unit]
    /// Description=Test Service
    /// After=network.target
    /// "#;
    /// let unit = SystemdUnit::from_str(input).unwrap();
    /// {
    ///     let mut section = unit.get_section("Unit").unwrap();
    ///     section.insert_after("Description", "Wants", "foo.service");
    /// }
    ///
    /// let section = unit.get_section("Unit").unwrap();
    /// let entries: Vec<_> = section.entries().collect();
    /// assert_eq!(entries[0].key(), Some("Description".to_string()));
    /// assert_eq!(entries[1].key(), Some("Wants".to_string()));
    /// assert_eq!(entries[2].key(), Some("After".to_string()));
    /// ```
    pub fn insert_after(&mut self, existing_key: &str, key: &str, value: &str) {
        let new_entry = Entry::new(key, value);

        // Find the first entry with the matching key
        let target_entry = self
            .entries()
            .find(|e| e.key().as_deref() == Some(existing_key));

        if let Some(entry) = target_entry {
            let insertion_index = entry.0.index() + 1;
            self.0
                .splice_children(insertion_index..insertion_index, vec![new_entry.0.into()]);
        }
        // If the key doesn't exist, do nothing
    }

    /// Set a space-separated list value for a key
    ///
    /// This is a convenience method for setting list-type directives
    /// (e.g., `Wants=`, `After=`). The values will be joined with spaces.
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SystemdUnit;
    /// # use std::str::FromStr;
    /// # let mut unit = SystemdUnit::from_str("[Unit]\n").unwrap();
    /// # let mut section = unit.get_section("Unit").unwrap();
    /// section.set_list("Wants", &["foo.service", "bar.service"]);
    /// // Results in: Wants=foo.service bar.service
    /// ```
    pub fn set_list(&mut self, key: &str, values: &[&str]) {
        let value = values.join(" ");
        self.set(key, &value);
    }

    /// Get a value parsed as a space-separated list
    ///
    /// This is a convenience method for getting list-type directives.
    /// If the key doesn't exist, returns an empty vector.
    pub fn get_list(&self, key: &str) -> Vec<String> {
        self.entries()
            .find(|e| e.key().as_deref() == Some(key))
            .map(|e| e.value_as_list())
            .unwrap_or_default()
    }

    /// Get a value parsed as a boolean
    ///
    /// Returns `None` if the key doesn't exist or if the value is not a valid boolean.
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SystemdUnit;
    /// # use std::str::FromStr;
    /// let unit = SystemdUnit::from_str("[Service]\nRemainAfterExit=yes\n").unwrap();
    /// let section = unit.get_section("Service").unwrap();
    /// assert_eq!(section.get_bool("RemainAfterExit"), Some(true));
    /// ```
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.entries()
            .find(|e| e.key().as_deref() == Some(key))
            .and_then(|e| e.value_as_bool())
    }

    /// Set a boolean value for a key
    ///
    /// This is a convenience method that formats the boolean as "yes" or "no".
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SystemdUnit;
    /// # use std::str::FromStr;
    /// let unit = SystemdUnit::from_str("[Service]\n").unwrap();
    /// let mut section = unit.get_section("Service").unwrap();
    /// section.set_bool("RemainAfterExit", true);
    /// assert_eq!(section.get("RemainAfterExit"), Some("yes".to_string()));
    /// ```
    pub fn set_bool(&mut self, key: &str, value: bool) {
        self.set(key, Entry::format_bool(value));
    }

    /// Remove the first entry with the given key
    pub fn remove(&mut self, key: &str) {
        // Find and remove the first entry with the matching key
        let entry_to_remove = self.0.children().find_map(|child| {
            let entry = Entry::cast(child)?;
            if entry.key().as_deref() == Some(key) {
                Some(entry)
            } else {
                None
            }
        });

        if let Some(entry) = entry_to_remove {
            entry.syntax().detach();
        }
    }

    /// Remove all entries with the given key
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

    /// Remove a specific value from entries with the given key
    ///
    /// This is useful for multi-value fields like `After=`, `Wants=`, etc.
    /// It handles space-separated values within a single entry and removes
    /// entire entries if they only contain the target value.
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SystemdUnit;
    /// # use std::str::FromStr;
    /// let input = r#"[Unit]
    /// After=network.target syslog.target
    /// After=remote-fs.target
    /// "#;
    /// let unit = SystemdUnit::from_str(input).unwrap();
    /// {
    ///     let mut section = unit.sections().next().unwrap();
    ///     section.remove_value("After", "syslog.target");
    /// }
    ///
    /// let section = unit.sections().next().unwrap();
    /// let all_after = section.get_all("After");
    /// assert_eq!(all_after.len(), 2);
    /// assert_eq!(all_after[0], "network.target");
    /// assert_eq!(all_after[1], "remote-fs.target");
    /// ```
    pub fn remove_value(&mut self, key: &str, value_to_remove: &str) {
        // Collect all entries with the matching key
        let entries_to_process: Vec<_> = self
            .entries()
            .filter(|e| e.key().as_deref() == Some(key))
            .collect();

        for entry in entries_to_process {
            // Get the current value as a list
            let current_list = entry.value_as_list();

            // Filter out the target value
            let new_list: Vec<_> = current_list
                .iter()
                .filter(|v| v.as_str() != value_to_remove)
                .map(|s| s.as_str())
                .collect();

            if new_list.is_empty() {
                // Remove the entire entry if no values remain
                entry.syntax().detach();
            } else if new_list.len() < current_list.len() {
                // Some values were removed but some remain
                // Create a new entry with the filtered values
                let new_entry = Entry::new(key, &new_list.join(" "));

                // Replace the old entry with the new one
                let index = entry.0.index();
                self.0
                    .splice_children(index..index + 1, vec![new_entry.0.into()]);
            }
            // else: the value wasn't found in this entry, no change needed
        }
    }

    /// Remove entries matching a predicate
    ///
    /// This provides a flexible way to remove entries based on arbitrary conditions.
    /// The predicate receives the entry's key and value and should return `true` for
    /// entries that should be removed.
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SystemdUnit;
    /// # use std::str::FromStr;
    /// let input = r#"[Unit]
    /// After=network.target syslog.target
    /// Wants=foo.service
    /// After=remote-fs.target
    /// "#;
    /// let unit = SystemdUnit::from_str(input).unwrap();
    /// {
    ///     let mut section = unit.sections().next().unwrap();
    ///     section.remove_entries_where(|key, value| {
    ///         key == "After" && value.split_whitespace().any(|v| v == "syslog.target")
    ///     });
    /// }
    ///
    /// let section = unit.sections().next().unwrap();
    /// let all_after = section.get_all("After");
    /// assert_eq!(all_after.len(), 1);
    /// assert_eq!(all_after[0], "remote-fs.target");
    /// assert_eq!(section.get("Wants"), Some("foo.service".to_string()));
    /// ```
    pub fn remove_entries_where<F>(&mut self, mut predicate: F)
    where
        F: FnMut(&str, &str) -> bool,
    {
        // Collect all entries to remove first (can't mutate while iterating)
        let entries_to_remove: Vec<_> = self
            .entries()
            .filter(|entry| {
                if let (Some(key), Some(value)) = (entry.key(), entry.value()) {
                    predicate(&key, &value)
                } else {
                    false
                }
            })
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

impl AstNode for Section {
    type Language = Lang;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SECTION
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::SECTION {
            Some(Section(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Unescape a string by processing C-style escape sequences
fn unescape_string(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some('\'') => result.push('\''),
                Some('x') => {
                    // Hexadecimal byte: \xhh
                    let hex: String = chars.by_ref().take(2).collect();
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        result.push(byte as char);
                    } else {
                        // Invalid escape, keep as-is
                        result.push('\\');
                        result.push('x');
                        result.push_str(&hex);
                    }
                }
                Some('u') => {
                    // Unicode codepoint: \unnnn
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(unicode_char) = char::from_u32(code) {
                            result.push(unicode_char);
                        } else {
                            // Invalid codepoint, keep as-is
                            result.push('\\');
                            result.push('u');
                            result.push_str(&hex);
                        }
                    } else {
                        // Invalid escape, keep as-is
                        result.push('\\');
                        result.push('u');
                        result.push_str(&hex);
                    }
                }
                Some('U') => {
                    // Unicode codepoint: \Unnnnnnnn
                    let hex: String = chars.by_ref().take(8).collect();
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(unicode_char) = char::from_u32(code) {
                            result.push(unicode_char);
                        } else {
                            // Invalid codepoint, keep as-is
                            result.push('\\');
                            result.push('U');
                            result.push_str(&hex);
                        }
                    } else {
                        // Invalid escape, keep as-is
                        result.push('\\');
                        result.push('U');
                        result.push_str(&hex);
                    }
                }
                Some(c) if c.is_ascii_digit() => {
                    // Octal byte: \nnn (up to 3 digits)
                    let mut octal = String::from(c);
                    for _ in 0..2 {
                        if let Some(&next_ch) = chars.peek() {
                            if next_ch.is_ascii_digit() && next_ch < '8' {
                                octal.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                    }
                    if let Ok(byte) = u8::from_str_radix(&octal, 8) {
                        result.push(byte as char);
                    } else {
                        // Invalid escape, keep as-is
                        result.push('\\');
                        result.push_str(&octal);
                    }
                }
                Some(c) => {
                    // Unknown escape sequence, keep the backslash
                    result.push('\\');
                    result.push(c);
                }
                None => {
                    // Backslash at end of string
                    result.push('\\');
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Escape a string for use in systemd unit files
fn escape_string(s: &str) -> String {
    let mut result = String::new();

    for ch in s.chars() {
        match ch {
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\t' => result.push_str("\\t"),
            '\r' => result.push_str("\\r"),
            '"' => result.push_str("\\\""),
            _ => result.push(ch),
        }
    }

    result
}

/// Remove quotes from a string if present
///
/// According to systemd specification, quotes (both double and single) are
/// removed when processing values. This function handles:
/// - Removing matching outer quotes
/// - Preserving whitespace inside quotes
/// - Handling escaped quotes inside quoted strings
fn unquote_string(s: &str) -> String {
    let trimmed = s.trim();

    if trimmed.len() < 2 {
        return trimmed.to_string();
    }

    let first = trimmed.chars().next();
    let last = trimmed.chars().last();

    // Check if string is quoted with matching quotes
    if let (Some('"'), Some('"')) = (first, last) {
        // Remove outer quotes
        trimmed[1..trimmed.len() - 1].to_string()
    } else if let (Some('\''), Some('\'')) = (first, last) {
        // Remove outer quotes
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        // Not quoted, return as-is (but trimmed)
        trimmed.to_string()
    }
}

/// A key-value entry in a section
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

    /// Get the key name
    pub fn key(&self) -> Option<String> {
        let key_token = self
            .0
            .children_with_tokens()
            .find(|e| e.kind() == SyntaxKind::KEY)?;
        Some(key_token.as_token()?.text().to_string())
    }

    /// Get the value (handles line continuations)
    pub fn value(&self) -> Option<String> {
        // Find all VALUE tokens after EQUALS, handling line continuations
        let mut found_equals = false;
        let mut value_parts = Vec::new();

        for element in self.0.children_with_tokens() {
            match element.kind() {
                SyntaxKind::EQUALS => found_equals = true,
                SyntaxKind::VALUE if found_equals => {
                    value_parts.push(element.as_token()?.text().to_string());
                }
                SyntaxKind::LINE_CONTINUATION if found_equals => {
                    // Line continuation: backslash-newline is replaced with a space
                    // But don't add a space if the last value part already ends with whitespace
                    let should_add_space = value_parts
                        .last()
                        .map(|s| !s.ends_with(' ') && !s.ends_with('\t'))
                        .unwrap_or(true);
                    if should_add_space {
                        value_parts.push(" ".to_string());
                    }
                }
                SyntaxKind::WHITESPACE if found_equals && !value_parts.is_empty() => {
                    // Only include whitespace that's part of the value (after we've started collecting)
                    // Skip leading whitespace immediately after EQUALS
                    value_parts.push(element.as_token()?.text().to_string());
                }
                SyntaxKind::NEWLINE => break,
                _ => {}
            }
        }

        if value_parts.is_empty() {
            None
        } else {
            // Join all value parts (line continuations already converted to spaces)
            Some(value_parts.join(""))
        }
    }

    /// Get the raw value as it appears in the file (including line continuations)
    pub fn raw_value(&self) -> Option<String> {
        let mut found_equals = false;
        let mut value_parts = Vec::new();

        for element in self.0.children_with_tokens() {
            match element.kind() {
                SyntaxKind::EQUALS => found_equals = true,
                SyntaxKind::VALUE if found_equals => {
                    value_parts.push(element.as_token()?.text().to_string());
                }
                SyntaxKind::LINE_CONTINUATION if found_equals => {
                    value_parts.push(element.as_token()?.text().to_string());
                }
                SyntaxKind::WHITESPACE if found_equals => {
                    value_parts.push(element.as_token()?.text().to_string());
                }
                SyntaxKind::NEWLINE => break,
                _ => {}
            }
        }

        if value_parts.is_empty() {
            None
        } else {
            Some(value_parts.join(""))
        }
    }

    /// Get the value with escape sequences processed
    ///
    /// This processes C-style escape sequences as defined in the systemd specification:
    /// - `\n` - newline
    /// - `\t` - tab
    /// - `\r` - carriage return
    /// - `\\` - backslash
    /// - `\"` - double quote
    /// - `\'` - single quote
    /// - `\xhh` - hexadecimal byte (2 digits)
    /// - `\nnn` - octal byte (3 digits)
    /// - `\unnnn` - Unicode codepoint (4 hex digits)
    /// - `\Unnnnnnnn` - Unicode codepoint (8 hex digits)
    pub fn unescape_value(&self) -> Option<String> {
        let value = self.value()?;
        Some(unescape_string(&value))
    }

    /// Escape a string value for use in systemd unit files
    ///
    /// This escapes special characters that need escaping in systemd values:
    /// - backslash (`\`) becomes `\\`
    /// - newline (`\n`) becomes `\n`
    /// - tab (`\t`) becomes `\t`
    /// - carriage return (`\r`) becomes `\r`
    /// - double quote (`"`) becomes `\"`
    pub fn escape_value(value: &str) -> String {
        escape_string(value)
    }

    /// Check if the value is quoted (starts and ends with matching quotes)
    ///
    /// Returns the quote character if the value is quoted, None otherwise.
    /// Systemd supports both double quotes (`"`) and single quotes (`'`).
    pub fn is_quoted(&self) -> Option<char> {
        let value = self.value()?;
        let trimmed = value.trim();

        if trimmed.len() < 2 {
            return None;
        }

        let first = trimmed.chars().next()?;
        let last = trimmed.chars().last()?;

        if (first == '"' || first == '\'') && first == last {
            Some(first)
        } else {
            None
        }
    }

    /// Get the value with quotes removed (if present)
    ///
    /// According to systemd specification, quotes are removed when processing values.
    /// This method returns the value with outer quotes stripped if present.
    pub fn unquoted_value(&self) -> Option<String> {
        let value = self.value()?;
        Some(unquote_string(&value))
    }

    /// Get the value with quotes preserved as they appear in the file
    ///
    /// This is useful when you want to preserve the exact quoting style.
    pub fn quoted_value(&self) -> Option<String> {
        // This is the same as value() - just provided for clarity
        self.value()
    }

    /// Parse the value as a space-separated list
    ///
    /// Many systemd directives use space-separated lists (e.g., `Wants=`,
    /// `After=`, `Before=`). This method splits the value on whitespace
    /// and returns a vector of strings.
    ///
    /// Empty values return an empty vector.
    pub fn value_as_list(&self) -> Vec<String> {
        let value = match self.unquoted_value() {
            Some(v) => v,
            None => return Vec::new(),
        };

        value.split_whitespace().map(|s| s.to_string()).collect()
    }

    /// Parse the value as a boolean
    ///
    /// According to systemd specification, boolean values accept:
    /// - Positive: `1`, `yes`, `true`, `on`
    /// - Negative: `0`, `no`, `false`, `off`
    ///
    /// Returns `None` if the value is not a valid boolean or if the entry has no value.
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SystemdUnit;
    /// # use std::str::FromStr;
    /// let unit = SystemdUnit::from_str("[Service]\nRemainAfterExit=yes\n").unwrap();
    /// let section = unit.get_section("Service").unwrap();
    /// let entry = section.entries().next().unwrap();
    /// assert_eq!(entry.value_as_bool(), Some(true));
    /// ```
    pub fn value_as_bool(&self) -> Option<bool> {
        let value = self.unquoted_value()?;
        let value_lower = value.trim().to_lowercase();

        match value_lower.as_str() {
            "1" | "yes" | "true" | "on" => Some(true),
            "0" | "no" | "false" | "off" => Some(false),
            _ => None,
        }
    }

    /// Format a boolean value for use in systemd unit files
    ///
    /// This converts a boolean to the canonical systemd format:
    /// - `true` becomes `"yes"`
    /// - `false` becomes `"no"`
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::Entry;
    /// assert_eq!(Entry::format_bool(true), "yes");
    /// assert_eq!(Entry::format_bool(false), "no");
    /// ```
    pub fn format_bool(value: bool) -> &'static str {
        if value {
            "yes"
        } else {
            "no"
        }
    }

    /// Expand systemd specifiers in the value
    ///
    /// This replaces systemd specifiers like `%i`, `%u`, `%h` with their
    /// values from the provided context.
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::{SystemdUnit, SpecifierContext};
    /// # use std::str::FromStr;
    /// let unit = SystemdUnit::from_str("[Service]\nWorkingDirectory=/var/lib/%i\n").unwrap();
    /// let section = unit.get_section("Service").unwrap();
    /// let entry = section.entries().next().unwrap();
    ///
    /// let mut ctx = SpecifierContext::new();
    /// ctx.set("i", "myinstance");
    ///
    /// assert_eq!(entry.expand_specifiers(&ctx), Some("/var/lib/myinstance".to_string()));
    /// ```
    pub fn expand_specifiers(
        &self,
        context: &crate::specifier::SpecifierContext,
    ) -> Option<String> {
        let value = self.value()?;
        Some(context.expand(&value))
    }

    /// Set a new value for this entry, modifying it in place
    ///
    /// This replaces the entry's value while preserving its key and position
    /// in the section. This is useful when iterating over entries and modifying
    /// them selectively.
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SystemdUnit;
    /// # use std::str::FromStr;
    /// let input = r#"[Unit]
    /// After=network.target syslog.target
    /// Wants=foo.service
    /// After=remote-fs.target
    /// "#;
    /// let unit = SystemdUnit::from_str(input).unwrap();
    /// let section = unit.get_section("Unit").unwrap();
    ///
    /// for entry in section.entries() {
    ///     if entry.key().as_deref() == Some("After") {
    ///         let values = entry.value_as_list();
    ///         let filtered: Vec<_> = values.iter()
    ///             .filter(|v| v.as_str() != "syslog.target")
    ///             .map(|s| s.as_str())
    ///             .collect();
    ///         entry.set_value(&filtered.join(" "));
    ///     }
    /// }
    ///
    /// let section = unit.get_section("Unit").unwrap();
    /// assert_eq!(section.get_all("After"), vec!["network.target", "remote-fs.target"]);
    /// ```
    pub fn set_value(&self, new_value: &str) {
        let key = self.key().expect("Entry should have a key");
        let new_entry = Entry::new(&key, new_value);

        // Get parent and replace this entry
        let parent = self.0.parent().expect("Entry should have a parent");
        let index = self.0.index();
        parent.splice_children(index..index + 1, vec![new_entry.0.into()]);
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
        let input = r#"[Unit]
Description=Test Service
After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        assert_eq!(unit.sections().count(), 1);

        let section = unit.sections().next().unwrap();
        assert_eq!(section.name(), Some("Unit".to_string()));
        assert_eq!(section.get("Description"), Some("Test Service".to_string()));
        assert_eq!(section.get("After"), Some("network.target".to_string()));
    }

    #[test]
    fn test_parse_with_comments() {
        let input = r#"# Top comment
[Unit]
# Comment before description
Description=Test Service
; Semicolon comment
After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        assert_eq!(unit.sections().count(), 1);

        let section = unit.sections().next().unwrap();
        assert_eq!(section.get("Description"), Some("Test Service".to_string()));
    }

    #[test]
    fn test_parse_multiple_sections() {
        let input = r#"[Unit]
Description=Test Service

[Service]
Type=simple
ExecStart=/usr/bin/test

[Install]
WantedBy=multi-user.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        assert_eq!(unit.sections().count(), 3);

        let unit_section = unit.get_section("Unit").unwrap();
        assert_eq!(
            unit_section.get("Description"),
            Some("Test Service".to_string())
        );

        let service_section = unit.get_section("Service").unwrap();
        assert_eq!(service_section.get("Type"), Some("simple".to_string()));
        assert_eq!(
            service_section.get("ExecStart"),
            Some("/usr/bin/test".to_string())
        );

        let install_section = unit.get_section("Install").unwrap();
        assert_eq!(
            install_section.get("WantedBy"),
            Some("multi-user.target".to_string())
        );
    }

    #[test]
    fn test_parse_with_spaces() {
        let input = "[Unit]\nDescription = Test Service\n";
        let unit = SystemdUnit::from_str(input).unwrap();

        let section = unit.sections().next().unwrap();
        assert_eq!(section.get("Description"), Some("Test Service".to_string()));
    }

    #[test]
    fn test_line_continuation() {
        let input = "[Service]\nExecStart=/bin/echo \\\n  hello world\n";
        let unit = SystemdUnit::from_str(input).unwrap();

        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();
        assert_eq!(entry.key(), Some("ExecStart".to_string()));
        // Line continuation: backslash is replaced with space
        assert_eq!(entry.value(), Some("/bin/echo   hello world".to_string()));
    }

    #[test]
    fn test_lossless_roundtrip() {
        let input = r#"# Comment
[Unit]
Description=Test Service
After=network.target

[Service]
Type=simple
ExecStart=/usr/bin/test
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let output = unit.text();
        assert_eq!(input, output);
    }

    #[test]
    fn test_set_value() {
        let input = r#"[Unit]
Description=Test Service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.set("Description", "Updated Service");
        }

        let section = unit.sections().next().unwrap();
        assert_eq!(
            section.get("Description"),
            Some("Updated Service".to_string())
        );
    }

    #[test]
    fn test_add_new_entry() {
        let input = r#"[Unit]
Description=Test Service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.set("After", "network.target");
        }

        let section = unit.sections().next().unwrap();
        assert_eq!(section.get("Description"), Some("Test Service".to_string()));
        assert_eq!(section.get("After"), Some("network.target".to_string()));
    }

    #[test]
    fn test_multiple_values_same_key() {
        let input = r#"[Unit]
Wants=foo.service
Wants=bar.service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();

        // get() returns the first value
        assert_eq!(section.get("Wants"), Some("foo.service".to_string()));

        // get_all() returns all values
        let all_wants = section.get_all("Wants");
        assert_eq!(all_wants.len(), 2);
        assert_eq!(all_wants[0], "foo.service");
        assert_eq!(all_wants[1], "bar.service");
    }

    #[test]
    fn test_add_multiple_entries() {
        let input = r#"[Unit]
Description=Test Service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.add("Wants", "foo.service");
            section.add("Wants", "bar.service");
        }

        let section = unit.sections().next().unwrap();
        let all_wants = section.get_all("Wants");
        assert_eq!(all_wants.len(), 2);
        assert_eq!(all_wants[0], "foo.service");
        assert_eq!(all_wants[1], "bar.service");
    }

    #[test]
    fn test_remove_entry() {
        let input = r#"[Unit]
Description=Test Service
After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove("After");
        }

        let section = unit.sections().next().unwrap();
        assert_eq!(section.get("Description"), Some("Test Service".to_string()));
        assert_eq!(section.get("After"), None);
    }

    #[test]
    fn test_remove_all_entries() {
        let input = r#"[Unit]
Wants=foo.service
Wants=bar.service
Description=Test
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove_all("Wants");
        }

        let section = unit.sections().next().unwrap();
        assert_eq!(section.get_all("Wants").len(), 0);
        assert_eq!(section.get("Description"), Some("Test".to_string()));
    }

    #[test]
    fn test_unescape_basic() {
        let input = r#"[Unit]
Description=Test\nService
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        assert_eq!(entry.value(), Some("Test\\nService".to_string()));
        assert_eq!(entry.unescape_value(), Some("Test\nService".to_string()));
    }

    #[test]
    fn test_unescape_all_escapes() {
        let input = r#"[Unit]
Value=\n\t\r\\\"\'\x41\101\u0041\U00000041
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        let unescaped = entry.unescape_value().unwrap();
        // \n = newline, \t = tab, \r = carriage return, \\ = backslash
        // \" = quote, \' = single quote
        // \x41 = 'A', \101 = 'A', \u0041 = 'A', \U00000041 = 'A'
        assert_eq!(unescaped, "\n\t\r\\\"'AAAA");
    }

    #[test]
    fn test_unescape_unicode() {
        let input = r#"[Unit]
Value=Hello\u0020World\U0001F44D
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        let unescaped = entry.unescape_value().unwrap();
        // \u0020 = space, \U0001F44D = 👍
        assert_eq!(unescaped, "Hello World👍");
    }

    #[test]
    fn test_escape_value() {
        let text = "Hello\nWorld\t\"Test\"\\Path";
        let escaped = Entry::escape_value(text);
        assert_eq!(escaped, "Hello\\nWorld\\t\\\"Test\\\"\\\\Path");
    }

    #[test]
    fn test_escape_unescape_roundtrip() {
        let original = "Test\nwith\ttabs\rand\"quotes\"\\backslash";
        let escaped = Entry::escape_value(original);
        let unescaped = unescape_string(&escaped);
        assert_eq!(original, unescaped);
    }

    #[test]
    fn test_unescape_invalid_sequences() {
        // Invalid escape sequences should be kept as-is or handled gracefully
        let input = r#"[Unit]
Value=\z\xFF\u12\U1234
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        let unescaped = entry.unescape_value().unwrap();
        // \z is unknown, \xFF has only 2 chars but needs hex, \u12 and \U1234 are incomplete
        assert!(unescaped.contains("\\z"));
    }

    #[test]
    fn test_quoted_double_quotes() {
        let input = r#"[Unit]
Description="Test Service"
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        assert_eq!(entry.value(), Some("\"Test Service\"".to_string()));
        assert_eq!(entry.quoted_value(), Some("\"Test Service\"".to_string()));
        assert_eq!(entry.unquoted_value(), Some("Test Service".to_string()));
        assert_eq!(entry.is_quoted(), Some('"'));
    }

    #[test]
    fn test_quoted_single_quotes() {
        let input = r#"[Unit]
Description='Test Service'
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        assert_eq!(entry.value(), Some("'Test Service'".to_string()));
        assert_eq!(entry.unquoted_value(), Some("Test Service".to_string()));
        assert_eq!(entry.is_quoted(), Some('\''));
    }

    #[test]
    fn test_quoted_with_whitespace() {
        let input = r#"[Unit]
Description="  Test Service  "
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        // Quotes preserve internal whitespace
        assert_eq!(entry.unquoted_value(), Some("  Test Service  ".to_string()));
    }

    #[test]
    fn test_unquoted_value() {
        let input = r#"[Unit]
Description=Test Service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        assert_eq!(entry.value(), Some("Test Service".to_string()));
        assert_eq!(entry.unquoted_value(), Some("Test Service".to_string()));
        assert_eq!(entry.is_quoted(), None);
    }

    #[test]
    fn test_mismatched_quotes() {
        let input = r#"[Unit]
Description="Test Service'
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        // Mismatched quotes should not be considered quoted
        assert_eq!(entry.is_quoted(), None);
        assert_eq!(entry.unquoted_value(), Some("\"Test Service'".to_string()));
    }

    #[test]
    fn test_empty_quotes() {
        let input = r#"[Unit]
Description=""
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        assert_eq!(entry.is_quoted(), Some('"'));
        assert_eq!(entry.unquoted_value(), Some("".to_string()));
    }

    #[test]
    fn test_value_as_list() {
        let input = r#"[Unit]
After=network.target remote-fs.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        let list = entry.value_as_list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], "network.target");
        assert_eq!(list[1], "remote-fs.target");
    }

    #[test]
    fn test_value_as_list_single() {
        let input = r#"[Unit]
After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        let list = entry.value_as_list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0], "network.target");
    }

    #[test]
    fn test_value_as_list_empty() {
        let input = r#"[Unit]
After=
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        let list = entry.value_as_list();
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn test_value_as_list_with_extra_whitespace() {
        let input = r#"[Unit]
After=  network.target   remote-fs.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();

        let list = entry.value_as_list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], "network.target");
        assert_eq!(list[1], "remote-fs.target");
    }

    #[test]
    fn test_section_get_list() {
        let input = r#"[Unit]
After=network.target remote-fs.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();

        let list = section.get_list("After");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], "network.target");
        assert_eq!(list[1], "remote-fs.target");
    }

    #[test]
    fn test_section_get_list_missing() {
        let input = r#"[Unit]
Description=Test
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();

        let list = section.get_list("After");
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn test_section_set_list() {
        let input = r#"[Unit]
Description=Test
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.set_list("After", &["network.target", "remote-fs.target"]);
        }

        let section = unit.sections().next().unwrap();
        let list = section.get_list("After");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], "network.target");
        assert_eq!(list[1], "remote-fs.target");
    }

    #[test]
    fn test_section_set_list_replaces() {
        let input = r#"[Unit]
After=foo.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.set_list("After", &["network.target", "remote-fs.target"]);
        }

        let section = unit.sections().next().unwrap();
        let list = section.get_list("After");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], "network.target");
        assert_eq!(list[1], "remote-fs.target");
    }

    #[test]
    fn test_value_as_bool_positive() {
        let inputs = vec!["yes", "true", "1", "on", "YES", "True", "ON"];

        for input_val in inputs {
            let input = format!("[Service]\nRemainAfterExit={}\n", input_val);
            let unit = SystemdUnit::from_str(&input).unwrap();
            let section = unit.sections().next().unwrap();
            let entry = section.entries().next().unwrap();
            assert_eq!(
                entry.value_as_bool(),
                Some(true),
                "Failed for input: {}",
                input_val
            );
        }
    }

    #[test]
    fn test_value_as_bool_negative() {
        let inputs = vec!["no", "false", "0", "off", "NO", "False", "OFF"];

        for input_val in inputs {
            let input = format!("[Service]\nRemainAfterExit={}\n", input_val);
            let unit = SystemdUnit::from_str(&input).unwrap();
            let section = unit.sections().next().unwrap();
            let entry = section.entries().next().unwrap();
            assert_eq!(
                entry.value_as_bool(),
                Some(false),
                "Failed for input: {}",
                input_val
            );
        }
    }

    #[test]
    fn test_value_as_bool_invalid() {
        let input = r#"[Service]
RemainAfterExit=maybe
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();
        assert_eq!(entry.value_as_bool(), None);
    }

    #[test]
    fn test_value_as_bool_with_whitespace() {
        let input = r#"[Service]
RemainAfterExit=  yes
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();
        assert_eq!(entry.value_as_bool(), Some(true));
    }

    #[test]
    fn test_format_bool() {
        assert_eq!(Entry::format_bool(true), "yes");
        assert_eq!(Entry::format_bool(false), "no");
    }

    #[test]
    fn test_section_get_bool() {
        let input = r#"[Service]
RemainAfterExit=yes
Type=simple
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.sections().next().unwrap();

        assert_eq!(section.get_bool("RemainAfterExit"), Some(true));
        assert_eq!(section.get_bool("Type"), None); // Not a boolean
        assert_eq!(section.get_bool("Missing"), None); // Doesn't exist
    }

    #[test]
    fn test_section_set_bool() {
        let input = r#"[Service]
Type=simple
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.set_bool("RemainAfterExit", true);
            section.set_bool("PrivateTmp", false);
        }

        let section = unit.sections().next().unwrap();
        assert_eq!(section.get("RemainAfterExit"), Some("yes".to_string()));
        assert_eq!(section.get("PrivateTmp"), Some("no".to_string()));
        assert_eq!(section.get_bool("RemainAfterExit"), Some(true));
        assert_eq!(section.get_bool("PrivateTmp"), Some(false));
    }

    #[test]
    fn test_add_entry_with_trailing_whitespace() {
        // Section with trailing blank lines
        let input = r#"[Unit]
Description=Test Service

"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.add("After", "network.target");
        }

        let output = unit.text();
        // New entry should be added immediately after the last entry, not after whitespace
        let expected = r#"[Unit]
Description=Test Service
After=network.target

"#;
        assert_eq!(output, expected);
    }

    #[test]
    fn test_set_new_entry_with_trailing_whitespace() {
        // Section with trailing blank lines
        let input = r#"[Unit]
Description=Test Service

"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.set("After", "network.target");
        }

        let output = unit.text();
        // New entry should be added immediately after the last entry, not after whitespace
        let expected = r#"[Unit]
Description=Test Service
After=network.target

"#;
        assert_eq!(output, expected);
    }

    #[test]
    fn test_remove_value_from_space_separated_list() {
        let input = r#"[Unit]
After=network.target syslog.target remote-fs.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove_value("After", "syslog.target");
        }

        let section = unit.sections().next().unwrap();
        assert_eq!(
            section.get("After"),
            Some("network.target remote-fs.target".to_string())
        );
    }

    #[test]
    fn test_remove_value_removes_entire_entry() {
        let input = r#"[Unit]
After=syslog.target
Description=Test
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove_value("After", "syslog.target");
        }

        let section = unit.sections().next().unwrap();
        assert_eq!(section.get("After"), None);
        assert_eq!(section.get("Description"), Some("Test".to_string()));
    }

    #[test]
    fn test_remove_value_from_multiple_entries() {
        let input = r#"[Unit]
After=network.target syslog.target
After=remote-fs.target
After=syslog.target multi-user.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove_value("After", "syslog.target");
        }

        let section = unit.sections().next().unwrap();
        let all_after = section.get_all("After");
        assert_eq!(all_after.len(), 3);
        assert_eq!(all_after[0], "network.target");
        assert_eq!(all_after[1], "remote-fs.target");
        assert_eq!(all_after[2], "multi-user.target");
    }

    #[test]
    fn test_remove_value_not_found() {
        let input = r#"[Unit]
After=network.target remote-fs.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove_value("After", "nonexistent.target");
        }

        let section = unit.sections().next().unwrap();
        // Should remain unchanged
        assert_eq!(
            section.get("After"),
            Some("network.target remote-fs.target".to_string())
        );
    }

    #[test]
    fn test_remove_value_preserves_order() {
        let input = r#"[Unit]
Description=Test Service
After=network.target syslog.target
Wants=foo.service
After=remote-fs.target
Requires=bar.service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove_value("After", "syslog.target");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();

        // Verify order is preserved
        assert_eq!(entries[0].key(), Some("Description".to_string()));
        assert_eq!(entries[1].key(), Some("After".to_string()));
        assert_eq!(entries[1].value(), Some("network.target".to_string()));
        assert_eq!(entries[2].key(), Some("Wants".to_string()));
        assert_eq!(entries[3].key(), Some("After".to_string()));
        assert_eq!(entries[3].value(), Some("remote-fs.target".to_string()));
        assert_eq!(entries[4].key(), Some("Requires".to_string()));
    }

    #[test]
    fn test_remove_value_key_not_found() {
        let input = r#"[Unit]
Description=Test Service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove_value("After", "network.target");
        }

        // Should not panic or error, just no-op
        let section = unit.sections().next().unwrap();
        assert_eq!(section.get("Description"), Some("Test Service".to_string()));
        assert_eq!(section.get("After"), None);
    }

    #[test]
    fn test_entry_set_value_basic() {
        let input = r#"[Unit]
After=network.target
Description=Test
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.get_section("Unit").unwrap();

        for entry in section.entries() {
            if entry.key().as_deref() == Some("After") {
                entry.set_value("remote-fs.target");
            }
        }

        let section = unit.get_section("Unit").unwrap();
        assert_eq!(section.get("After"), Some("remote-fs.target".to_string()));
        assert_eq!(section.get("Description"), Some("Test".to_string()));
    }

    #[test]
    fn test_entry_set_value_preserves_order() {
        let input = r#"[Unit]
Description=Test Service
After=network.target syslog.target
Wants=foo.service
After=remote-fs.target
Requires=bar.service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.get_section("Unit").unwrap();

        for entry in section.entries() {
            if entry.key().as_deref() == Some("After") {
                let values = entry.value_as_list();
                let filtered: Vec<_> = values
                    .iter()
                    .filter(|v| v.as_str() != "syslog.target")
                    .map(|s| s.as_str())
                    .collect();
                if !filtered.is_empty() {
                    entry.set_value(&filtered.join(" "));
                }
            }
        }

        let section = unit.get_section("Unit").unwrap();
        let entries: Vec<_> = section.entries().collect();

        // Verify order is preserved
        assert_eq!(entries[0].key(), Some("Description".to_string()));
        assert_eq!(entries[1].key(), Some("After".to_string()));
        assert_eq!(entries[1].value(), Some("network.target".to_string()));
        assert_eq!(entries[2].key(), Some("Wants".to_string()));
        assert_eq!(entries[3].key(), Some("After".to_string()));
        assert_eq!(entries[3].value(), Some("remote-fs.target".to_string()));
        assert_eq!(entries[4].key(), Some("Requires".to_string()));
    }

    #[test]
    fn test_entry_set_value_multiple_entries() {
        let input = r#"[Unit]
After=network.target
After=syslog.target
After=remote-fs.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.get_section("Unit").unwrap();

        // Collect entries first to avoid iterator invalidation issues
        let entries_to_modify: Vec<_> = section
            .entries()
            .filter(|e| e.key().as_deref() == Some("After"))
            .collect();

        // Modify all After entries
        for entry in entries_to_modify {
            let old_value = entry.value().unwrap();
            entry.set_value(&format!("{} multi-user.target", old_value));
        }

        let section = unit.get_section("Unit").unwrap();
        let all_after = section.get_all("After");
        assert_eq!(all_after.len(), 3);
        assert_eq!(all_after[0], "network.target multi-user.target");
        assert_eq!(all_after[1], "syslog.target multi-user.target");
        assert_eq!(all_after[2], "remote-fs.target multi-user.target");
    }

    #[test]
    fn test_entry_set_value_with_empty_string() {
        let input = r#"[Unit]
After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        let section = unit.get_section("Unit").unwrap();

        for entry in section.entries() {
            if entry.key().as_deref() == Some("After") {
                entry.set_value("");
            }
        }

        let section = unit.get_section("Unit").unwrap();
        assert_eq!(section.get("After"), Some("".to_string()));
    }

    #[test]
    fn test_remove_entries_where_basic() {
        let input = r#"[Unit]
After=network.target
Wants=foo.service
After=syslog.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove_entries_where(|key, _value| key == "After");
        }

        let section = unit.sections().next().unwrap();
        assert_eq!(section.get_all("After").len(), 0);
        assert_eq!(section.get("Wants"), Some("foo.service".to_string()));
    }

    #[test]
    fn test_remove_entries_where_with_value_check() {
        let input = r#"[Unit]
After=network.target syslog.target
Wants=foo.service
After=remote-fs.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove_entries_where(|key, value| {
                key == "After" && value.split_whitespace().any(|v| v == "syslog.target")
            });
        }

        let section = unit.sections().next().unwrap();
        let all_after = section.get_all("After");
        assert_eq!(all_after.len(), 1);
        assert_eq!(all_after[0], "remote-fs.target");
        assert_eq!(section.get("Wants"), Some("foo.service".to_string()));
    }

    #[test]
    fn test_remove_entries_where_preserves_order() {
        let input = r#"[Unit]
Description=Test Service
After=network.target
Wants=foo.service
After=syslog.target
Requires=bar.service
After=remote-fs.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove_entries_where(|key, value| key == "After" && value.contains("syslog"));
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();

        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].key(), Some("Description".to_string()));
        assert_eq!(entries[1].key(), Some("After".to_string()));
        assert_eq!(entries[1].value(), Some("network.target".to_string()));
        assert_eq!(entries[2].key(), Some("Wants".to_string()));
        assert_eq!(entries[3].key(), Some("Requires".to_string()));
        assert_eq!(entries[4].key(), Some("After".to_string()));
        assert_eq!(entries[4].value(), Some("remote-fs.target".to_string()));
    }

    #[test]
    fn test_remove_entries_where_no_matches() {
        let input = r#"[Unit]
After=network.target
Wants=foo.service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove_entries_where(|key, _value| key == "Requires");
        }

        let section = unit.sections().next().unwrap();
        assert_eq!(section.get("After"), Some("network.target".to_string()));
        assert_eq!(section.get("Wants"), Some("foo.service".to_string()));
    }

    #[test]
    fn test_remove_entries_where_all_entries() {
        let input = r#"[Unit]
After=network.target
Wants=foo.service
Requires=bar.service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.remove_entries_where(|_key, _value| true);
        }

        let section = unit.sections().next().unwrap();
        assert_eq!(section.entries().count(), 0);
    }

    #[test]
    fn test_remove_entries_where_complex_predicate() {
        let input = r#"[Unit]
After=network.target
After=syslog.target remote-fs.target
Wants=foo.service
After=multi-user.target
Requires=bar.service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            // Remove After entries with multiple space-separated values
            section.remove_entries_where(|key, value| {
                key == "After" && value.split_whitespace().count() > 1
            });
        }

        let section = unit.sections().next().unwrap();
        let all_after = section.get_all("After");
        assert_eq!(all_after.len(), 2);
        assert_eq!(all_after[0], "network.target");
        assert_eq!(all_after[1], "multi-user.target");
    }

    #[test]
    fn test_insert_at_beginning() {
        let input = r#"[Unit]
Description=Test Service
After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_at(0, "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key(), Some("Wants".to_string()));
        assert_eq!(entries[0].value(), Some("foo.service".to_string()));
        assert_eq!(entries[1].key(), Some("Description".to_string()));
        assert_eq!(entries[2].key(), Some("After".to_string()));
    }

    #[test]
    fn test_insert_at_middle() {
        let input = r#"[Unit]
Description=Test Service
After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_at(1, "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key(), Some("Description".to_string()));
        assert_eq!(entries[1].key(), Some("Wants".to_string()));
        assert_eq!(entries[1].value(), Some("foo.service".to_string()));
        assert_eq!(entries[2].key(), Some("After".to_string()));
    }

    #[test]
    fn test_insert_at_end() {
        let input = r#"[Unit]
Description=Test Service
After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_at(2, "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key(), Some("Description".to_string()));
        assert_eq!(entries[1].key(), Some("After".to_string()));
        assert_eq!(entries[2].key(), Some("Wants".to_string()));
        assert_eq!(entries[2].value(), Some("foo.service".to_string()));
    }

    #[test]
    fn test_insert_at_beyond_end() {
        let input = r#"[Unit]
Description=Test Service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_at(100, "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key(), Some("Description".to_string()));
        assert_eq!(entries[1].key(), Some("Wants".to_string()));
        assert_eq!(entries[1].value(), Some("foo.service".to_string()));
    }

    #[test]
    fn test_insert_at_empty_section() {
        let input = r#"[Unit]
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_at(0, "Description", "Test Service");
        }

        let section = unit.sections().next().unwrap();
        assert_eq!(section.get("Description"), Some("Test Service".to_string()));
    }

    #[test]
    fn test_insert_before_basic() {
        let input = r#"[Unit]
Description=Test Service
After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_before("After", "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key(), Some("Description".to_string()));
        assert_eq!(entries[1].key(), Some("Wants".to_string()));
        assert_eq!(entries[1].value(), Some("foo.service".to_string()));
        assert_eq!(entries[2].key(), Some("After".to_string()));
    }

    #[test]
    fn test_insert_before_first_entry() {
        let input = r#"[Unit]
Description=Test Service
After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_before("Description", "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key(), Some("Wants".to_string()));
        assert_eq!(entries[0].value(), Some("foo.service".to_string()));
        assert_eq!(entries[1].key(), Some("Description".to_string()));
        assert_eq!(entries[2].key(), Some("After".to_string()));
    }

    #[test]
    fn test_insert_before_nonexistent_key() {
        let input = r#"[Unit]
Description=Test Service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_before("After", "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key(), Some("Description".to_string()));
    }

    #[test]
    fn test_insert_before_multiple_occurrences() {
        let input = r#"[Unit]
After=network.target
After=syslog.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_before("After", "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key(), Some("Wants".to_string()));
        assert_eq!(entries[1].key(), Some("After".to_string()));
        assert_eq!(entries[1].value(), Some("network.target".to_string()));
        assert_eq!(entries[2].key(), Some("After".to_string()));
        assert_eq!(entries[2].value(), Some("syslog.target".to_string()));
    }

    #[test]
    fn test_insert_after_basic() {
        let input = r#"[Unit]
Description=Test Service
After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_after("Description", "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key(), Some("Description".to_string()));
        assert_eq!(entries[1].key(), Some("Wants".to_string()));
        assert_eq!(entries[1].value(), Some("foo.service".to_string()));
        assert_eq!(entries[2].key(), Some("After".to_string()));
    }

    #[test]
    fn test_insert_after_last_entry() {
        let input = r#"[Unit]
Description=Test Service
After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_after("After", "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key(), Some("Description".to_string()));
        assert_eq!(entries[1].key(), Some("After".to_string()));
        assert_eq!(entries[2].key(), Some("Wants".to_string()));
        assert_eq!(entries[2].value(), Some("foo.service".to_string()));
    }

    #[test]
    fn test_insert_after_nonexistent_key() {
        let input = r#"[Unit]
Description=Test Service
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_after("After", "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key(), Some("Description".to_string()));
    }

    #[test]
    fn test_insert_after_multiple_occurrences() {
        let input = r#"[Unit]
After=network.target
After=syslog.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_after("After", "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key(), Some("After".to_string()));
        assert_eq!(entries[0].value(), Some("network.target".to_string()));
        assert_eq!(entries[1].key(), Some("Wants".to_string()));
        assert_eq!(entries[2].key(), Some("After".to_string()));
        assert_eq!(entries[2].value(), Some("syslog.target".to_string()));
    }

    #[test]
    fn test_insert_preserves_whitespace() {
        let input = r#"[Unit]
Description=Test Service

After=network.target
"#;
        let unit = SystemdUnit::from_str(input).unwrap();
        {
            let mut section = unit.sections().next().unwrap();
            section.insert_at(1, "Wants", "foo.service");
        }

        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key(), Some("Description".to_string()));
        assert_eq!(entries[1].key(), Some("Wants".to_string()));
        assert_eq!(entries[2].key(), Some("After".to_string()));

        let expected = r#"[Unit]
Description=Test Service

Wants=foo.service
After=network.target
"#;
        assert_eq!(unit.text(), expected);
    }

    #[test]
    fn test_line_col() {
        let text = r#"[Unit]
Description=Test Service
After=network.target

[Service]
Type=simple
ExecStart=/usr/bin/test
Environment="FOO=bar"
"#;
        let unit = SystemdUnit::from_str(text).unwrap();

        // Test SystemdUnit line numbers (should start at line 0)
        assert_eq!(unit.line(), 0);
        assert_eq!(unit.column(), 0);
        assert_eq!(unit.line_col(), (0, 0));

        // Test section line numbers
        let sections: Vec<_> = unit.sections().collect();
        assert_eq!(sections.len(), 2);

        // First section [Unit] starts at line 0
        assert_eq!(sections[0].line(), 0);
        assert_eq!(sections[0].column(), 0);
        assert_eq!(sections[0].line_col(), (0, 0));

        // Second section [Service] starts at line 4 (after the empty line)
        assert_eq!(sections[1].line(), 4);
        assert_eq!(sections[1].column(), 0);
        assert_eq!(sections[1].line_col(), (4, 0));

        // Test entry line numbers
        let unit_entries: Vec<_> = sections[0].entries().collect();
        assert_eq!(unit_entries.len(), 2);
        assert_eq!(unit_entries[0].line(), 1); // Description=Test Service
        assert_eq!(unit_entries[0].column(), 0); // Start of line
        assert_eq!(unit_entries[1].line(), 2); // After=network.target
        assert_eq!(unit_entries[1].column(), 0); // Start of line

        let service_entries: Vec<_> = sections[1].entries().collect();
        assert_eq!(service_entries.len(), 3);
        assert_eq!(service_entries[0].line(), 5); // Type=simple
        assert_eq!(service_entries[0].column(), 0); // Start of line
        assert_eq!(service_entries[1].line(), 6); // ExecStart=...
        assert_eq!(service_entries[1].column(), 0); // Start of line
        assert_eq!(service_entries[2].line(), 7); // Environment=...
        assert_eq!(service_entries[2].column(), 0); // Start of line

        // Test line_col() method
        assert_eq!(unit_entries[0].line_col(), (1, 0));
        assert_eq!(service_entries[2].line_col(), (7, 0));
    }

    #[test]
    fn test_line_col_multiline() {
        // Test with line continuations
        let text = r#"[Unit]
Description=A long \
value that spans \
multiple lines
After=network.target
"#;
        let unit = SystemdUnit::from_str(text).unwrap();
        let section = unit.sections().next().unwrap();
        let entries: Vec<_> = section.entries().collect();

        assert_eq!(entries.len(), 2);
        // First entry starts at line 1
        assert_eq!(entries[0].line(), 1);
        assert_eq!(entries[0].column(), 0);

        // Second entry starts at line 4 (after the multi-line value)
        assert_eq!(entries[1].line(), 4);
        assert_eq!(entries[1].column(), 0);
    }

    #[test]
    fn test_leading_whitespace_error() {
        // Test that leading whitespace on a key is reported as an error
        let input = r#"[Unit]
Description=Test Service
 ConditionVirtualization=microsoft
"#;
        let result = SystemdUnit::from_str(input);

        // The parser should not hang and should report an error
        assert!(
            result.is_err(),
            "Expected parse error for leading whitespace"
        );

        match result {
            Err(Error::ParseError(err)) => {
                assert!(
                    err.0
                        .iter()
                        .any(|e| e.contains("unexpected whitespace at start of line")),
                    "Expected error about leading whitespace, got: {:?}",
                    err.0
                );
            }
            _ => panic!("Expected ParseError, got: {:?}", result),
        }
    }

    #[test]
    fn test_leading_whitespace_does_not_hang() {
        // Test that leading whitespace doesn't cause an infinite loop
        let input = r#"[Unit]
Description=Test Service
 After=network.target
Wants=foo.service
"#;
        // This should complete without hanging and return an error
        let result = SystemdUnit::from_str(input);
        assert!(
            result.is_err(),
            "Expected parse error for leading whitespace"
        );
    }

    #[test]
    fn test_leading_whitespace_multiple_lines() {
        // Test that multiple lines with leading whitespace are all reported as errors
        let input = r#"[Unit]
Description=Test Service
 After=network.target
 Wants=foo.service
 Requires=bar.service
"#;
        let result = SystemdUnit::from_str(input);
        assert!(
            result.is_err(),
            "Expected parse error for leading whitespace"
        );

        match result {
            Err(Error::ParseError(err)) => {
                // Should have errors for each line with leading whitespace
                assert!(
                    err.0.len() >= 3,
                    "Expected at least 3 errors for 3 lines with leading whitespace, got {}",
                    err.0.len()
                );
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_valid_continuation_line() {
        // Test that valid continuation lines (after backslash) work correctly
        let input = r#"[Service]
ExecStart=/bin/echo \
  hello world
"#;
        let unit = SystemdUnit::from_str(input).unwrap();

        // Continuation lines should work fine
        let section = unit.sections().next().unwrap();
        let entry = section.entries().next().unwrap();
        assert_eq!(entry.key(), Some("ExecStart".to_string()));
    }

    #[test]
    fn test_blank_lines_with_whitespace() {
        // Test that blank lines containing only whitespace don't cause issues
        let input = "[Unit]\nDescription=Test\n  \t  \nAfter=network.target\n";
        let unit = SystemdUnit::from_str(input).unwrap();

        // Should parse successfully
        let section = unit.sections().next().unwrap();
        assert_eq!(section.get("Description"), Some("Test".to_string()));
        assert_eq!(section.get("After"), Some("network.target".to_string()));
    }
}
