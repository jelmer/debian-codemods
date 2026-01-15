// Copyright (C) 2025 Jelmer Vernooij
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program; if not, write to the Free Software
// Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

//! Lossless YAML editor using rowan for syntax tree representation
//!
//! This crate provides a Rust API for editing YAML files while preserving
//! formatting, comments, and whitespace.

use indexmap::IndexMap;
use rowan::{GreenNode, GreenNodeBuilder};
use std::cell::RefCell;
use std::io::Write;
use std::path::Path;

// ============================================================================
// Rowan-based YAML Syntax Tree
// ============================================================================

/// Syntax kinds for YAML tokens and nodes
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    // Tokens
    WHITESPACE = 0,
    COMMENT,
    NEWLINE,
    KEY,
    COLON,
    VALUE,
    DASH,      // - for list items
    DOC_START, // ---
    DOC_END,   // ...

    // JSON-specific tokens
    LBRACE,   // {
    RBRACE,   // }
    LBRACKET, // [
    RBRACKET, // ]
    COMMA,    // ,
    STRING,   // "quoted string"

    // Nodes
    ROOT,
    DOCUMENT,
    MAPPING,
    ENTRY,
    SEQUENCE,
    SEQUENCE_ITEM,
    KEY_ITEM,   // Node wrapping key text in an ENTRY
    VALUE_ITEM, // Node wrapping value text/structure in an ENTRY

    ERROR,
}

use rowan::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum YamlLanguage {}

impl Language for YamlLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 <= SyntaxKind::ERROR as u16);
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

type SyntaxNode = rowan::SyntaxNode<YamlLanguage>;

// ============================================================================
// Lexer
// ============================================================================

fn lex(input: &str) -> Vec<(SyntaxKind, &str)> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();

    while let Some((start, ch)) = chars.next() {
        let kind = match ch {
            '#' => {
                // Comment - consume until newline
                let mut end = start + 1;
                while let Some(&(pos, c)) = chars.peek() {
                    if c == '\n' {
                        break;
                    }
                    chars.next();
                    end = pos + c.len_utf8();
                }
                (SyntaxKind::COMMENT, &input[start..end])
            }
            '%' => {
                // YAML directive - consume until newline
                let mut end = start + 1;
                while let Some(&(pos, c)) = chars.peek() {
                    if c == '\n' {
                        break;
                    }
                    chars.next();
                    end = pos + c.len_utf8();
                }
                (SyntaxKind::COMMENT, &input[start..end]) // Treat as comment for parsing purposes
            }
            '\n' => (SyntaxKind::NEWLINE, &input[start..start + 1]),
            ' ' | '\t' => {
                // Whitespace
                let mut end = start + 1;
                while let Some(&(pos, c)) = chars.peek() {
                    if c != ' ' && c != '\t' {
                        break;
                    }
                    chars.next();
                    end = pos + 1;
                }
                (SyntaxKind::WHITESPACE, &input[start..end])
            }
            ':' => (SyntaxKind::COLON, &input[start..start + 1]),
            '{' => (SyntaxKind::LBRACE, &input[start..start + 1]),
            '}' => (SyntaxKind::RBRACE, &input[start..start + 1]),
            '[' => (SyntaxKind::LBRACKET, &input[start..start + 1]),
            ']' => (SyntaxKind::RBRACKET, &input[start..start + 1]),
            ',' => (SyntaxKind::COMMA, &input[start..start + 1]),
            '"' => {
                // Quoted string - consume until closing quote
                let mut end = start + 1;
                let mut escaped = false;
                while let Some(&(pos, c)) = chars.peek() {
                    chars.next();
                    end = pos + c.len_utf8();
                    if escaped {
                        escaped = false;
                        continue;
                    }
                    if c == '\\' {
                        escaped = true;
                        continue;
                    }
                    if c == '"' {
                        break;
                    }
                }
                (SyntaxKind::STRING, &input[start..end])
            }
            '-' => {
                // Check if this is --- (doc start) or just -
                if let Some(&(_, '-')) = chars.peek() {
                    chars.next(); // consume second -
                    if let Some(&(_, '-')) = chars.peek() {
                        chars.next(); // consume third -
                        (SyntaxKind::DOC_START, &input[start..start + 3])
                    } else {
                        // Just two dashes, treat as value
                        (SyntaxKind::VALUE, &input[start..start + 2])
                    }
                } else {
                    (SyntaxKind::DASH, &input[start..start + 1])
                }
            }
            _ => {
                // Default: consume as value until we hit special character
                let mut end = start + ch.len_utf8();
                while let Some(&(pos, c)) = chars.peek() {
                    if c == '\n' || c == '#' {
                        break;
                    }
                    // Check if this is a colon that should be treated as a separator
                    if c == ':' {
                        // Look ahead to see what follows the colon
                        let mut temp_chars = chars.clone();
                        temp_chars.next(); // skip the ':'
                        if let Some(&(_, next_c)) = temp_chars.peek() {
                            // Break if colon is followed by space/tab/newline
                            // This makes it a YAML key-value separator
                            if next_c == ' ' || next_c == '\t' || next_c == '\n' {
                                break;
                            }
                            // Otherwise, colon is part of the value (like in URLs or malformed keys like "Repository:")
                        } else {
                            // Colon at end of input is a separator
                            break;
                        }
                    }
                    chars.next();
                    end = pos + c.len_utf8();
                }
                let text = &input[start..end];

                // Check if line starts with this text (ignoring leading whitespace) - if so, it's a KEY
                // This is a simplification - we'll refine in parser
                (SyntaxKind::VALUE, text)
            }
        };

        tokens.push(kind);
    }

    tokens
}

// ============================================================================
// Parser
// ============================================================================

struct Parser<'a> {
    tokens: Vec<(SyntaxKind, &'a str)>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
}

impl<'a> Parser<'a> {
    fn new(tokens: Vec<(SyntaxKind, &'a str)>) -> Self {
        Self {
            tokens,
            pos: 0,
            builder: GreenNodeBuilder::new(),
        }
    }

    fn current(&self) -> Option<(SyntaxKind, &'a str)> {
        self.tokens.get(self.pos).copied()
    }

    fn bump(&mut self) {
        if let Some((kind, text)) = self.current() {
            self.builder.token(YamlLanguage::kind_to_raw(kind), text);
            self.pos += 1;
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(
            self.current(),
            Some((SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE, _))
        ) {
            self.bump();
        }
    }

    fn parse(mut self) -> GreenNode {
        self.builder
            .start_node(YamlLanguage::kind_to_raw(SyntaxKind::ROOT));

        // Skip leading whitespace and comments
        while let Some((kind, _)) = self.current() {
            match kind {
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT => {
                    self.bump();
                }
                _ => break,
            }
        }

        // Parse document
        if self.current().is_some() {
            self.parse_document();
        }

        self.builder.finish_node();
        self.builder.finish()
    }

    fn parse_document(&mut self) {
        self.builder
            .start_node(YamlLanguage::kind_to_raw(SyntaxKind::DOCUMENT));

        // Check for document start marker
        if matches!(self.current(), Some((SyntaxKind::DOC_START, _))) {
            self.bump();
            self.skip_whitespace();
        }

        // Check if this is JSON format, YAML sequence, or YAML mapping
        match self.current() {
            Some((SyntaxKind::LBRACE, _)) => self.parse_json_object(),
            Some((SyntaxKind::LBRACKET, _)) => self.parse_json_array(),
            Some((SyntaxKind::DASH, _)) => self.parse_sequence(),
            _ => self.parse_mapping(),
        }

        // Consume any trailing whitespace/newlines to preserve them in the CST
        while let Some((kind, _)) = self.current() {
            if matches!(kind, SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE) {
                self.bump();
            } else {
                break;
            }
        }

        self.builder.finish_node();
    }

    fn parse_sequence(&mut self) {
        self.builder
            .start_node(YamlLanguage::kind_to_raw(SyntaxKind::SEQUENCE));

        while matches!(self.current(), Some((SyntaxKind::DASH, _))) {
            // Parse sequence item
            self.builder
                .start_node(YamlLanguage::kind_to_raw(SyntaxKind::SEQUENCE_ITEM));

            // Bump the dash
            self.bump();

            // Skip whitespace after dash
            if matches!(self.current(), Some((SyntaxKind::WHITESPACE, _))) {
                self.bump();
            }

            // Check if this is a key-value entry or just a plain value
            // Look ahead to see if there's a colon after the first value token
            let is_entry = if matches!(self.current(), Some((SyntaxKind::VALUE, _))) {
                // Save position
                let saved_pos = self.pos;

                // Skip the value token
                self.pos += 1;

                // Skip optional whitespace
                while matches!(self.current(), Some((SyntaxKind::WHITESPACE, _))) {
                    self.pos += 1;
                }

                // Check if next token is a colon
                let has_colon = matches!(self.current(), Some((SyntaxKind::COLON, _)));

                // Restore position
                self.pos = saved_pos;

                has_colon
            } else {
                false
            };

            if is_entry {
                // This is a key-value pair like "- Author: Name"
                self.parse_entry();
            } else {
                // This is a plain value - consume everything until newline
                while let Some((kind, _)) = self.current() {
                    if matches!(kind, SyntaxKind::NEWLINE | SyntaxKind::COMMENT) {
                        break;
                    }
                    self.bump();
                }
            }

            self.builder.finish_node();

            // Skip trailing whitespace/newlines
            while matches!(
                self.current(),
                Some((
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT,
                    _
                ))
            ) {
                self.bump();
            }
        }

        self.builder.finish_node();
    }

    fn parse_mapping(&mut self) {
        self.builder
            .start_node(YamlLanguage::kind_to_raw(SyntaxKind::MAPPING));

        while let Some((kind, _text)) = self.current() {
            match kind {
                SyntaxKind::COMMENT | SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {
                    self.bump();
                }
                SyntaxKind::VALUE => {
                    // This might be a key - check if colon follows
                    self.parse_entry();
                }
                _ => {
                    self.bump();
                }
            }
        }

        self.builder.finish_node();
    }

    fn parse_entry(&mut self) {
        self.builder
            .start_node(YamlLanguage::kind_to_raw(SyntaxKind::ENTRY));

        // Parse key - wrap in KEY_ITEM node
        if let Some((SyntaxKind::VALUE, text)) = self.current() {
            self.builder
                .start_node(YamlLanguage::kind_to_raw(SyntaxKind::KEY_ITEM));
            self.builder
                .token(YamlLanguage::kind_to_raw(SyntaxKind::KEY), text.trim());
            self.builder.finish_node(); // Finish KEY_ITEM
            self.pos += 1;
        }

        // Skip whitespace
        while matches!(self.current(), Some((SyntaxKind::WHITESPACE, _))) {
            self.bump();
        }

        // Expect colon
        if matches!(self.current(), Some((SyntaxKind::COLON, _))) {
            self.bump();
        }

        // Skip whitespace after colon
        while matches!(self.current(), Some((SyntaxKind::WHITESPACE, _))) {
            self.bump();
        }

        // Check if there's a value on the same line
        let has_inline_value = matches!(
            self.current(),
            Some((kind, _)) if !matches!(kind, SyntaxKind::NEWLINE | SyntaxKind::COMMENT)
        );

        if has_inline_value {
            // Wrap the value in a VALUE_ITEM node so we can replace it with splice_children
            self.builder
                .start_node(YamlLanguage::kind_to_raw(SyntaxKind::VALUE_ITEM));

            // Parse value (everything until newline or comment)
            while let Some((kind, _)) = self.current() {
                if matches!(kind, SyntaxKind::NEWLINE | SyntaxKind::COMMENT) {
                    break;
                }
                self.bump();
            }

            self.builder.finish_node(); // Finish VALUE_ITEM node
        } else {
            // No inline value - check if there's indented content following
            // Consume the newline first
            if matches!(self.current(), Some((SyntaxKind::NEWLINE, _))) {
                self.bump();
            }

            // Check if next line starts with whitespace (indented content)
            if matches!(self.current(), Some((SyntaxKind::WHITESPACE, _))) {
                // Look ahead to determine if this is a sequence or mapping
                let saved_pos = self.pos;
                self.pos += 1; // skip whitespace

                let is_sequence = matches!(self.current(), Some((SyntaxKind::DASH, _)));
                self.pos = saved_pos; // restore position

                // Wrap nested structure in VALUE_ITEM for consistency
                self.builder
                    .start_node(YamlLanguage::kind_to_raw(SyntaxKind::VALUE_ITEM));

                if is_sequence {
                    // Skip the leading whitespace
                    self.bump();
                    // Parse as sequence
                    self.parse_sequence();
                } else {
                    // Parse indented content as a nested mapping
                    self.parse_nested_mapping();
                }

                self.builder.finish_node(); // Finish VALUE_ITEM
                                            // Return early - nested structure already consumed the trailing newline
                self.builder.finish_node(); // Finish ENTRY
                return;
            }
        }

        // Consume trailing newline
        if matches!(self.current(), Some((SyntaxKind::NEWLINE, _))) {
            self.bump();
        }

        self.builder.finish_node();
    }

    fn parse_nested_mapping(&mut self) {
        self.builder
            .start_node(YamlLanguage::kind_to_raw(SyntaxKind::MAPPING));

        // Parse indented entries
        while let Some((kind, _text)) = self.current() {
            match kind {
                SyntaxKind::WHITESPACE => {
                    // Check if this is indentation at the start of a line
                    // If we see whitespace followed by a value token, it's an indented entry
                    self.bump();

                    if matches!(self.current(), Some((SyntaxKind::VALUE, _))) {
                        self.parse_entry();
                    }
                }
                SyntaxKind::VALUE => {
                    // Non-indented entry means we're done with the nested mapping
                    break;
                }
                SyntaxKind::COMMENT => {
                    self.bump();
                }
                SyntaxKind::NEWLINE => {
                    self.bump();
                }
                _ => {
                    // Stop parsing nested content when we hit something else
                    break;
                }
            }
        }

        self.builder.finish_node();
    }

    fn parse_json_object(&mut self) {
        self.builder
            .start_node(YamlLanguage::kind_to_raw(SyntaxKind::MAPPING));

        // Consume opening brace
        self.bump(); // {
        self.skip_whitespace();

        // Parse key-value pairs
        while !matches!(self.current(), Some((SyntaxKind::RBRACE, _)) | None) {
            // Skip whitespace/newlines
            while matches!(
                self.current(),
                Some((SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE, _))
            ) {
                self.bump();
            }

            // Check for closing brace
            if matches!(self.current(), Some((SyntaxKind::RBRACE, _)) | None) {
                break;
            }

            // Parse entry
            self.builder
                .start_node(YamlLanguage::kind_to_raw(SyntaxKind::ENTRY));

            // Parse key (should be a quoted string)
            if let Some((SyntaxKind::STRING, _)) = self.current() {
                self.builder
                    .start_node(YamlLanguage::kind_to_raw(SyntaxKind::KEY_ITEM));
                // Keep the STRING token as-is (with quotes) for lossless editing
                self.bump();
                self.builder.finish_node(); // KEY_ITEM
            }

            self.skip_whitespace();

            // Expect colon
            if matches!(self.current(), Some((SyntaxKind::COLON, _))) {
                self.bump();
            }

            self.skip_whitespace();

            // Parse value
            self.builder
                .start_node(YamlLanguage::kind_to_raw(SyntaxKind::VALUE_ITEM));
            self.parse_json_value();
            self.builder.finish_node(); // VALUE_ITEM

            self.builder.finish_node(); // ENTRY

            self.skip_whitespace();

            // Consume comma if present
            if matches!(self.current(), Some((SyntaxKind::COMMA, _))) {
                self.bump();
            }

            self.skip_whitespace();
        }

        // Consume closing brace
        if matches!(self.current(), Some((SyntaxKind::RBRACE, _))) {
            self.bump();
        }

        self.builder.finish_node(); // MAPPING
    }

    fn parse_json_array(&mut self) {
        self.builder
            .start_node(YamlLanguage::kind_to_raw(SyntaxKind::SEQUENCE));

        // Consume opening bracket
        self.bump(); // [
        self.skip_whitespace();

        // Parse array items
        while !matches!(self.current(), Some((SyntaxKind::RBRACKET, _)) | None) {
            // Skip whitespace/newlines
            while matches!(
                self.current(),
                Some((SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE, _))
            ) {
                self.bump();
            }

            // Check for closing bracket
            if matches!(self.current(), Some((SyntaxKind::RBRACKET, _)) | None) {
                break;
            }

            // Parse item
            self.builder
                .start_node(YamlLanguage::kind_to_raw(SyntaxKind::SEQUENCE_ITEM));

            self.parse_json_value();

            self.builder.finish_node(); // SEQUENCE_ITEM

            self.skip_whitespace();

            // Consume comma if present
            if matches!(self.current(), Some((SyntaxKind::COMMA, _))) {
                self.bump();
            }

            self.skip_whitespace();
        }

        // Consume closing bracket
        if matches!(self.current(), Some((SyntaxKind::RBRACKET, _))) {
            self.bump();
        }

        self.builder.finish_node(); // SEQUENCE
    }

    fn parse_json_value(&mut self) {
        match self.current() {
            Some((SyntaxKind::STRING, _)) => {
                // Keep the STRING token as-is (with quotes) for lossless editing
                self.bump();
            }
            Some((SyntaxKind::LBRACE, _)) => {
                // Nested object
                self.parse_json_object();
            }
            Some((SyntaxKind::LBRACKET, _)) => {
                // Nested array
                self.parse_json_array();
            }
            Some((SyntaxKind::VALUE, _)) => {
                // Unquoted value (number, boolean, null)
                self.bump();
            }
            _ => {
                // Unexpected, skip
                self.pos += 1;
            }
        }
    }
}

fn parse_yaml(input: &str) -> SyntaxNode {
    let tokens = lex(input);
    let parser = Parser::new(tokens);
    let green = parser.parse();
    SyntaxNode::new_root_mut(green)
}

// Check if a string value needs to be quoted in YAML block context
// (i.e., when used as a value in a key: value pair)
fn needs_quoting(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }

    // Check for leading/trailing whitespace
    if s.trim() != s {
        return true;
    }

    // Check if it could be interpreted as a number, boolean, or null
    if matches!(
        s,
        "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~"
    ) {
        return true;
    }
    if s.parse::<i64>().is_ok() || s.parse::<f64>().is_ok() {
        return true;
    }

    // Check if it starts with YAML indicators or special characters
    if let Some(first) = s.chars().next() {
        if matches!(
            first,
            '-' | '?'
                | ':'
                | '|'
                | '>'
                | '\''
                | '"'
                | '%'
                | '@'
                | '&'
                | '!'
                | '*'
                | '['
                | ']'
                | '{'
                | '}'
                | '#'
        ) {
            return true;
        }
    }

    // Check for colon followed by space (key-value separator)
    if s.contains(": ") {
        return true;
    }

    // Check for space followed by hash (comment marker)
    if s.contains(" #") {
        return true;
    }

    // Check for newlines
    if s.contains('\n') || s.contains('\r') {
        return true;
    }

    false
}

// Escape a string for JSON format (add quotes and escape special characters)
fn escape_json_string(s: &str) -> String {
    let mut result = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\u{0008}' => result.push_str("\\b"),
            '\u{000C}' => result.push_str("\\f"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            _ if ch.is_control() => {
                // Escape other control characters as \uXXXX
                result.push_str(&format!("\\u{:04x}", ch as u32));
            }
            _ => result.push(ch),
        }
    }
    result.push('"');
    result
}

// Unescape a JSON string (strip quotes and handle escape sequences)
fn unescape_json_string(s: &str) -> String {
    let s = s.trim_matches('"');
    let mut result = String::new();
    let mut chars = s.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                match next {
                    '"' => result.push('"'),
                    '\\' => result.push('\\'),
                    '/' => result.push('/'),
                    'b' => result.push('\u{0008}'),
                    'f' => result.push('\u{000C}'),
                    'n' => result.push('\n'),
                    'r' => result.push('\r'),
                    't' => result.push('\t'),
                    'u' => {
                        // Unicode escape: \uXXXX
                        let hex: String = chars.by_ref().take(4).collect();
                        if let Ok(code) = u32::from_str_radix(&hex, 16) {
                            if let Some(unicode_char) = char::from_u32(code) {
                                result.push(unicode_char);
                            }
                        }
                    }
                    _ => {
                        // Unknown escape, keep as-is
                        result.push('\\');
                        result.push(next);
                    }
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

// Build a new green node for an ENTRY with the given key and value
fn build_entry_green(key: &str, value: &Value) -> GreenNode {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::ENTRY));

    // KEY_ITEM node containing KEY token
    builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::KEY_ITEM));
    builder.token(YamlLanguage::kind_to_raw(SyntaxKind::KEY), key);
    builder.finish_node();

    // COLON token
    builder.token(YamlLanguage::kind_to_raw(SyntaxKind::COLON), ":");

    // WHITESPACE token
    builder.token(YamlLanguage::kind_to_raw(SyntaxKind::WHITESPACE), " ");

    // VALUE_ITEM node with properly structured value
    builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::VALUE_ITEM));
    build_value_nodes(&mut builder, value, false);
    builder.finish_node();

    // NEWLINE token
    builder.token(YamlLanguage::kind_to_raw(SyntaxKind::NEWLINE), "\n");

    builder.finish_node();
    builder.finish()
}

// Build just a VALUE_ITEM node (for replacing values)
// Build a VALUE_ITEM node with optional quoting
fn build_value_green_with_format(value: &Value, preserve_quotes: bool) -> GreenNode {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::VALUE_ITEM));
    build_value_nodes(&mut builder, value, preserve_quotes);
    builder.finish_node();
    builder.finish()
}

// Recursively build CST nodes for a value
// This builds what goes INSIDE a VALUE_ITEM node
fn build_value_nodes(builder: &mut GreenNodeBuilder, value: &Value, preserve_quotes: bool) {
    match value {
        Value::String(s) => {
            // Quote if: original was quoted OR new value requires quoting
            if preserve_quotes || needs_quoting(s) {
                // Use STRING token with quotes
                let escaped = escape_json_string(s);
                builder.token(YamlLanguage::kind_to_raw(SyntaxKind::STRING), &escaped);
            } else {
                // Use VALUE token without quotes
                builder.token(YamlLanguage::kind_to_raw(SyntaxKind::VALUE), s);
            }
        }
        Value::Int(i) => {
            builder.token(YamlLanguage::kind_to_raw(SyntaxKind::VALUE), &i.to_string());
        }
        Value::Float(f) => {
            builder.token(YamlLanguage::kind_to_raw(SyntaxKind::VALUE), &f.to_string());
        }
        Value::Bool(b) => {
            builder.token(YamlLanguage::kind_to_raw(SyntaxKind::VALUE), &b.to_string());
        }
        Value::Null => {
            builder.token(YamlLanguage::kind_to_raw(SyntaxKind::VALUE), "null");
        }
        Value::Map(map) => {
            // Build a nested MAPPING node
            builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::MAPPING));
            for (key, val) in map {
                builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::ENTRY));

                // KEY_ITEM
                builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::KEY_ITEM));
                builder.token(YamlLanguage::kind_to_raw(SyntaxKind::KEY), key);
                builder.finish_node();

                builder.token(YamlLanguage::kind_to_raw(SyntaxKind::COLON), ":");
                builder.token(YamlLanguage::kind_to_raw(SyntaxKind::WHITESPACE), " ");

                // VALUE_ITEM with recursive value
                builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::VALUE_ITEM));
                build_value_nodes(builder, val, preserve_quotes);
                builder.finish_node(); // VALUE_ITEM

                builder.token(YamlLanguage::kind_to_raw(SyntaxKind::NEWLINE), "\n");
                builder.finish_node(); // ENTRY
            }
            builder.finish_node(); // MAPPING
        }
        Value::List(items) => {
            // Build a SEQUENCE node
            builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::SEQUENCE));
            for item in items {
                builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::SEQUENCE_ITEM));
                builder.token(YamlLanguage::kind_to_raw(SyntaxKind::DASH), "-");
                builder.token(YamlLanguage::kind_to_raw(SyntaxKind::WHITESPACE), " ");

                // Recursively build the item value
                build_value_nodes(builder, item, preserve_quotes);

                builder.token(YamlLanguage::kind_to_raw(SyntaxKind::NEWLINE), "\n");
                builder.finish_node(); // SEQUENCE_ITEM
            }
            builder.finish_node(); // SEQUENCE
        }
    }
}

// ============================================================================
// Value Type
// ============================================================================

/// Document parsed with duplicate keys preserved
///
/// When YAML files contain duplicate keys, this structure preserves all values
/// for each key, allowing the caller to decide how to merge them.
#[derive(Debug, Clone)]
pub struct DuplicateKeyDocument {
    /// Map from field names to all their values (in order of appearance)
    pub fields: IndexMap<String, Vec<Value>>,
    /// Fields that appeared only once (for efficiency)
    pub unique_fields: IndexMap<String, Value>,
    /// Order of keys as they appeared in the original file
    pub key_order: Vec<String>,
}

/// Merge duplicate keys in a YAML file directly at the node level
///
/// This function works directly with YAML nodes (before type conversion) to merge
/// duplicate keys, preserving the original formatting and string representations.
/// For fields in `sequence_fields`, duplicate values are merged into a list.
/// For other fields, only the first value is kept.
///
/// Returns the list of field names that had duplicates, or None if no duplicates were found.
pub fn merge_duplicate_keys_in_place<P: AsRef<Path>>(
    path: P,
    sequence_fields: &[&str],
) -> Result<Option<Vec<String>>> {
    let path = path.as_ref();

    // Use parse_with_duplicates to detect and read duplicate keys
    let dup_doc = match YamlUpdater::parse_with_duplicates(path) {
        Ok(doc) => doc,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            // File doesn't exist - nothing to do
            return Ok(None);
        }
        Err(e) => return Err(e),
    };

    // Check if there are any duplicates
    if dup_doc.fields.is_empty() {
        return Ok(None);
    }

    // Build list of removed keys (one entry per duplicate removed, not including first)
    let mut removed_keys = Vec::new();
    for (key, values) in &dup_doc.fields {
        for _ in 1..values.len() {
            removed_keys.push(key.clone());
        }
    }

    // Build merged map combining duplicates with unique fields
    let mut merged_map = IndexMap::new();

    // First add all unique fields (preserving order)
    for key in &dup_doc.key_order {
        if let Some(value) = dup_doc.unique_fields.get(key) {
            merged_map.insert(key.clone(), value.clone());
        } else if let Some(values) = dup_doc.fields.get(key) {
            // This is a duplicate key - merge it
            if sequence_fields.contains(&key.as_str()) {
                // Merge into a sequence
                let mut merged_items = Vec::new();
                for val in values {
                    match val {
                        Value::List(items) => {
                            merged_items.extend(items.clone());
                        }
                        _ => {
                            merged_items.push(val.clone());
                        }
                    }
                }
                merged_map.insert(key.clone(), Value::List(merged_items));
            } else {
                // Keep only the first value
                merged_map.insert(key.clone(), values[0].clone());
            }
        }
    }

    // Check if original file has document marker
    let content = std::fs::read_to_string(path)?;
    let has_doc_marker = content.trim_start().starts_with("---");

    // Write back using write_yaml_file (must rewrite since we're removing duplicate entries)
    write_yaml_file(path, &Value::Map(merged_map), has_doc_marker)?;

    Ok(Some(removed_keys))
}

// Parse YAML considering indentation for nested structures
// Extract a Value from the CST by walking the tree
fn extract_value_from_cst(node: &SyntaxNode) -> Value {
    for child in node.children() {
        match child.kind() {
            SyntaxKind::DOCUMENT => return extract_value_from_cst(&child),
            SyntaxKind::SEQUENCE => return extract_sequence(&child),
            SyntaxKind::MAPPING => return extract_mapping(&child),
            _ => {}
        }
    }
    // Empty document
    Value::Map(IndexMap::new())
}

fn extract_sequence(node: &SyntaxNode) -> Value {
    let mut items = Vec::new();

    for child in node.children() {
        if child.kind() == SyntaxKind::SEQUENCE_ITEM {
            // Check if this item has an ENTRY, MAPPING, SEQUENCE, or is a plain value
            let mut has_structure = false;
            let mut value_text = String::new();

            for item_child in child.children_with_tokens() {
                match item_child {
                    rowan::NodeOrToken::Node(n) => {
                        match n.kind() {
                            SyntaxKind::ENTRY => {
                                // Single entry - wrap in a map
                                let (key, val) = extract_entry_pair(&n);
                                if !key.is_empty() {
                                    let mut map = IndexMap::new();
                                    map.insert(key, val);
                                    items.push(Value::Map(map));
                                }
                                has_structure = true;
                            }
                            SyntaxKind::MAPPING => {
                                items.push(extract_mapping(&n));
                                has_structure = true;
                            }
                            SyntaxKind::SEQUENCE => {
                                items.push(extract_sequence(&n));
                                has_structure = true;
                            }
                            _ => {}
                        }
                    }
                    rowan::NodeOrToken::Token(t) => {
                        // Collect tokens for plain value
                        if !has_structure {
                            match t.kind() {
                                SyntaxKind::DASH | SyntaxKind::WHITESPACE => {
                                    // Skip dash and leading whitespace
                                }
                                SyntaxKind::NEWLINE => {
                                    // Stop at newline
                                    break;
                                }
                                SyntaxKind::STRING => {
                                    // Unescape JSON strings
                                    value_text.push_str(&unescape_json_string(t.text()));
                                }
                                _ => {
                                    // Collect everything else as value text
                                    value_text.push_str(t.text());
                                }
                            }
                        }
                    }
                }
            }

            // If no structure was found, this is a plain value
            if !has_structure && !value_text.trim().is_empty() {
                items.push(parse_value(value_text.trim()));
            }
        }
    }

    Value::List(items)
}

fn extract_mapping(node: &SyntaxNode) -> Value {
    let mut map = IndexMap::new();

    for child in node.children() {
        if child.kind() == SyntaxKind::ENTRY {
            let (key, val) = extract_entry_pair(&child);
            if !key.is_empty() {
                map.insert(key, val);
            }
        }
    }

    Value::Map(map)
}

fn extract_entry_pair(entry_node: &SyntaxNode) -> (String, Value) {
    let mut key = String::new();
    let mut value = Value::Null;

    // Now ENTRY has child nodes: KEY_ITEM and VALUE_ITEM
    for child in entry_node.children() {
        match child.kind() {
            SyntaxKind::KEY_ITEM => {
                // Extract key text from KEY or STRING token inside KEY_ITEM
                for token in child.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(t) = token {
                        match t.kind() {
                            SyntaxKind::KEY => {
                                key = t.text().trim().to_string();
                            }
                            SyntaxKind::STRING => {
                                // Unescape JSON string (strip quotes and handle escapes)
                                key = unescape_json_string(t.text());
                            }
                            _ => {}
                        }
                    }
                }
            }
            SyntaxKind::VALUE_ITEM => {
                // Extract value from VALUE_ITEM
                // Could be inline text tokens or nested MAPPING/SEQUENCE
                let mut has_nested = false;
                for child_node in child.children() {
                    match child_node.kind() {
                        SyntaxKind::MAPPING => {
                            value = extract_mapping(&child_node);
                            has_nested = true;
                        }
                        SyntaxKind::SEQUENCE => {
                            value = extract_sequence(&child_node);
                            has_nested = true;
                        }
                        _ => {}
                    }
                }

                // If no nested structure, extract as text
                if !has_nested {
                    let mut value_text = String::new();
                    for token in child.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = token {
                            // Unescape JSON STRING tokens
                            if t.kind() == SyntaxKind::STRING {
                                value_text.push_str(&unescape_json_string(t.text()));
                            } else {
                                value_text.push_str(t.text());
                            }
                        }
                    }
                    value = if value_text.trim().is_empty() {
                        Value::Null
                    } else {
                        parse_value(value_text.trim())
                    };
                }
            }
            _ => {}
        }
    }

    (key, value)
}

// Extract entries from CST, collecting all occurrences of each key for duplicate detection
fn extract_entries(
    node: &SyntaxNode,
    by_key: &mut IndexMap<String, Vec<Value>>,
    key_order: &mut Vec<String>,
) {
    for child in node.children() {
        match child.kind() {
            SyntaxKind::DOCUMENT | SyntaxKind::MAPPING => {
                extract_entries(&child, by_key, key_order);
            }
            SyntaxKind::ENTRY => {
                let (key, val) = extract_entry_pair(&child);
                if !key.is_empty() {
                    if !by_key.contains_key(&key) {
                        key_order.push(key.clone());
                    }
                    by_key.entry(key).or_default().push(val);
                }
            }
            _ => {}
        }
    }
}

fn parse_value(text: &str) -> Value {
    let trimmed = text.trim();

    // Try to parse as different types
    if trimmed == "null" || trimmed.is_empty() {
        Value::Null
    } else if trimmed == "true" {
        Value::Bool(true)
    } else if trimmed == "false" {
        Value::Bool(false)
    } else if trimmed.starts_with('0')
        && trimmed.len() > 1
        && trimmed.chars().all(|c| c.is_ascii_digit())
    {
        // Preserve leading zeros as strings (like "01", "02")
        Value::String(trimmed.to_string())
    } else if trimmed.contains('.') && trimmed.chars().filter(|&c| c == '.').count() == 1 {
        // Keep decimal numbers as strings to preserve formatting like "1.0"
        Value::String(trimmed.to_string())
    } else if let Ok(i) = trimmed.parse::<i64>() {
        Value::Int(i)
    } else if let Ok(f) = trimmed.parse::<f64>() {
        Value::Float(f)
    } else {
        Value::String(trimmed.to_string())
    }
}

fn write_yaml_file(path: &Path, value: &Value, with_doc_marker: bool) -> Result<()> {
    let mut file = std::fs::File::create(path)?;

    if with_doc_marker {
        writeln!(file, "---")?;
    }

    write_value(&mut file, value, 0)?;

    Ok(())
}

fn write_value(file: &mut std::fs::File, value: &Value, indent: usize) -> Result<()> {
    match value {
        Value::Map(map) => {
            for (key, val) in map {
                write!(file, "{}", " ".repeat(indent))?;

                match val {
                    Value::List(_) => {
                        write!(file, "{}:", key)?;
                        writeln!(file)?;
                        // For lists at top level, don't add extra indentation
                        // List items start at the same column as their parent key
                        write_value(file, val, indent)?;
                    }
                    Value::Map(_) => {
                        write!(file, "{}:", key)?;
                        writeln!(file)?;
                        // For nested maps, indent by 1 space
                        write_value(file, val, indent + 1)?;
                    }
                    _ => {
                        write!(file, "{}: ", key)?;
                        write_value(file, val, 0)?;
                        writeln!(file)?;
                    }
                }
            }
        }
        Value::List(items) => {
            for item in items.iter() {
                write!(file, "{}- ", " ".repeat(indent))?;
                match item {
                    Value::Map(map) => {
                        // For maps in lists, write first key-value on same line as dash
                        let mut first = true;
                        for (key, val) in map {
                            if !first {
                                // Continuation lines need to align with first key (dash + space + indent)
                                write!(file, "{}", " ".repeat(indent + 2))?;
                            }
                            write!(file, "{}: ", key)?;
                            match val {
                                Value::Map(_) | Value::List(_) => {
                                    writeln!(file)?;
                                    write_value(file, val, indent + 4)?;
                                }
                                _ => {
                                    write_value(file, val, 0)?;
                                    writeln!(file)?;
                                }
                            }
                            first = false;
                        }
                    }
                    _ => {
                        write_value(file, item, 0)?;
                        writeln!(file)?;
                    }
                }
            }
        }
        Value::String(s) => write!(file, "{}", s)?,
        Value::Int(i) => write!(file, "{}", i)?,
        Value::Float(f) => write!(file, "{}", f)?,
        Value::Bool(b) => write!(file, "{}", b)?,
        Value::Null => write!(file, "null")?,
    }

    Ok(())
}

/// A YAML value that can be stored in the document
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A string value
    String(String),
    /// A null value
    Null,
    /// A boolean value
    Bool(bool),
    /// An integer value
    Int(i64),
    /// A float value
    Float(f64),
    /// A list of values
    List(Vec<Value>),
    /// A mapping of string keys to values
    Map(IndexMap<String, Value>),
}

/// Error type for YAML operations
#[derive(Debug)]
pub enum Error {
    /// YAML parsing or serialization error
    Yaml(String),
    /// File I/O error
    Io(std::io::Error),
    /// Value not found or wrong type
    ValueError(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Yaml(e) => write!(f, "YAML error: {}", e),
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::ValueError(s) => write!(f, "Value error: {}", s),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

// ============================================================================
// High-level API
// ============================================================================

/// A YAML document editor
pub struct YamlUpdater {
    path: std::path::PathBuf,
    remove_empty: bool,
    allow_duplicate_keys: bool,
}

impl YamlUpdater {
    /// Create a new YamlUpdater for the given path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(YamlUpdater {
            path: path.as_ref().to_path_buf(),
            remove_empty: true,
            allow_duplicate_keys: false,
        })
    }

    /// Write a Value directly to a YAML file, overwriting it completely
    ///
    /// If explicit_start is true, writes a `---` document start marker.
    pub fn write_file_with_options<P: AsRef<Path>>(
        path: P,
        value: Value,
        explicit_start: bool,
    ) -> Result<()> {
        write_yaml_file(path.as_ref(), &value, explicit_start)
    }

    /// Parse YAML file preserving duplicate keys
    ///
    /// This is a static method that parses a YAML file and preserves all values
    /// for duplicate keys, allowing the caller to decide how to merge them.
    pub fn parse_with_duplicates<P: AsRef<Path>>(path: P) -> Result<DuplicateKeyDocument> {
        let content = std::fs::read_to_string(path)?;

        if content.trim().is_empty() {
            return Ok(DuplicateKeyDocument {
                fields: IndexMap::new(),
                unique_fields: IndexMap::new(),
                key_order: Vec::new(),
            });
        }

        let tree = parse_yaml(&content);

        let mut by_key: IndexMap<String, Vec<Value>> = IndexMap::new();
        let mut key_order = Vec::new();

        extract_entries(&tree, &mut by_key, &mut key_order);

        let mut fields = IndexMap::new();
        let mut unique_fields = IndexMap::new();

        for (key, values) in by_key {
            if values.len() > 1 {
                fields.insert(key, values);
            } else {
                unique_fields.insert(key, values.into_iter().next().unwrap());
            }
        }

        Ok(DuplicateKeyDocument {
            fields,
            unique_fields,
            key_order,
        })
    }

    /// Set whether to remove the file if it becomes empty
    pub fn remove_empty(mut self, remove_empty: bool) -> Self {
        self.remove_empty = remove_empty;
        self
    }

    /// Set whether to allow duplicate keys
    pub fn allow_duplicate_keys(mut self, allow: bool) -> Self {
        self.allow_duplicate_keys = allow;
        self
    }

    /// Enter the context manager and load the YAML file
    pub fn open(&mut self) -> Result<YamlDocument> {
        let (orig_value, preamble, earlier_docs_csts, cst) = if self.path.exists() {
            let content = std::fs::read_to_string(&self.path)?;
            let lines: Vec<&str> = content.lines().collect();

            let mut preamble = Vec::new();
            let mut i = 0;

            // Extract preamble (empty lines, comments, and YAML directives at the start)
            while i < lines.len() {
                let line = lines[i];
                if line.trim().is_empty() || line.starts_with('#') || line.starts_with('%') {
                    preamble.push(line.to_string());
                    i += 1;
                } else {
                    break;
                }
            }

            // Parse everything from here on
            let mut content_rest = lines[i..].join("\n");
            // Preserve trailing newline if the original content had one
            if content.ends_with('\n') && !content_rest.ends_with('\n') {
                content_rest.push('\n');
            }

            // Check if this is a multi-document YAML file by counting --- markers
            let mut doc_start_positions: Vec<usize> = Vec::new();
            if content_rest.starts_with("---") {
                doc_start_positions.push(0);
            }
            for (pos, _) in content_rest.match_indices("\n---") {
                // pos points to the \n, so the next document starts at pos+1 (after the \n)
                doc_start_positions.push(pos + 1);
            }

            if doc_start_positions.len() <= 1 {
                // Single document (or no explicit markers)
                let tree = parse_yaml(&content_rest);
                let value = extract_value_from_cst(&tree);
                (value, preamble, vec![], tree)
            } else {
                // Multi-document file: split and parse each
                let mut doc_texts = Vec::new();
                for i in 0..doc_start_positions.len() {
                    let start = doc_start_positions[i];
                    let end = if i + 1 < doc_start_positions.len() {
                        doc_start_positions[i + 1] - 1 // Exclude the newline before next ---
                    } else {
                        content_rest.len()
                    };
                    let doc_text = &content_rest[start..end];
                    doc_texts.push(doc_text);
                }

                // Parse all but last as immutable CSTs
                let mut earlier_csts = Vec::new();
                for doc_text in &doc_texts[..doc_texts.len() - 1] {
                    let cst = parse_yaml(doc_text);
                    earlier_csts.push(cst);
                }

                // Parse last document as editable
                let last_text = doc_texts[doc_texts.len() - 1];
                let last_tree = parse_yaml(last_text);
                let last_value = extract_value_from_cst(&last_tree);

                (last_value, preamble, earlier_csts, last_tree)
            }
        } else {
            // For new files - create an empty MAPPING structure with document marker
            let mut builder = GreenNodeBuilder::new();
            builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::ROOT));
            builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::DOCUMENT));
            // Add document marker for new files
            builder.token(YamlLanguage::kind_to_raw(SyntaxKind::DOC_START), "---");
            builder.token(YamlLanguage::kind_to_raw(SyntaxKind::NEWLINE), "\n");
            builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::MAPPING));
            builder.finish_node(); // MAPPING
            builder.finish_node(); // DOCUMENT
            builder.finish_node(); // ROOT
            let empty_green = builder.finish();
            let empty_tree = SyntaxNode::new_root_mut(empty_green);
            (Value::Map(IndexMap::new()), Vec::new(), vec![], empty_tree)
        };

        let code = orig_value.clone();

        Ok(YamlDocument {
            path: self.path.clone(),
            orig: orig_value,
            code: RefCell::new(code),
            preamble,
            remove_empty: self.remove_empty,
            force_rewrite_flag: RefCell::new(false),
            earlier_docs_csts,
            cst: RefCell::new(cst),
        })
    }

    /// Close the context manager and save changes
    pub fn close(&mut self) -> Result<()> {
        // In native implementation, closing is handled by Drop
        Ok(())
    }
}

/// A YAML document that can be edited
pub struct YamlDocument {
    path: std::path::PathBuf,
    orig: Value,
    code: RefCell<Value>,
    preamble: Vec<String>,
    remove_empty: bool,
    force_rewrite_flag: RefCell<bool>,
    earlier_docs_csts: Vec<SyntaxNode>, // CSTs for earlier documents in multi-doc files (immutable)
    cst: RefCell<SyntaxNode>,           // The CST for the last/only document (mutable)
}

impl YamlDocument {
    /// Get a value from the YAML document by key
    pub fn get(&self, key: &str) -> Result<Option<Value>> {
        let code = self.code.borrow();
        if let Value::Map(ref map) = *code {
            Ok(map.get(key).cloned())
        } else {
            Ok(None)
        }
    }

    /// Set a value in the YAML document
    pub fn set(&self, key: &str, value: Value) -> Result<()> {
        // Update the code value (for get() to work)
        let mut code = self.code.borrow_mut();
        if let Value::Map(ref mut map) = *code {
            map.insert(key.to_string(), value.clone());
        } else {
            return Err(Error::ValueError("Document is not a map".to_string()));
        }
        drop(code); // Release borrow

        // Update the CST
        let cst = self.cst.borrow();

        // Find the MAPPING node in the CST - structure is ROOT -> DOCUMENT -> MAPPING
        let mapping = cst
            .descendants()
            .find(|n| n.kind() == SyntaxKind::MAPPING)
            .ok_or_else(|| Error::ValueError("No MAPPING node found".to_string()))?;

        // Find the ENTRY node for this key within the mapping
        let existing_entry = mapping.children().find(|child| {
            if child.kind() == SyntaxKind::ENTRY {
                // Extract the full key (including trailing colon if double colon case)
                let (entry_key, _) = extract_entry_pair(child);
                return entry_key == key;
            }
            false
        });

        if let Some(entry) = existing_entry {
            // Modify existing entry - replace only the VALUE_ITEM to preserve formatting
            // Find the VALUE_ITEM node within this entry
            if let Some(value_item) = entry
                .children()
                .find(|c| c.kind() == SyntaxKind::VALUE_ITEM)
            {
                let value_idx = value_item.index();

                // Preserve quoting style: check if the old value used quoted strings
                let had_quotes = value_item.descendants_with_tokens().any(|node_or_token| {
                    matches!(node_or_token, rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::STRING)
                });

                // Build a new VALUE_ITEM with the new value, preserving quoting if it was there
                let new_value_green = build_value_green_with_format(&value, had_quotes);
                let new_value_node = SyntaxNode::new_root_mut(new_value_green);

                // Replace only the VALUE_ITEM node (preserves key formatting, commas, etc.)
                entry.splice_children(value_idx..value_idx + 1, vec![new_value_node.into()]);
            } else {
                // Fallback: replace entire entry if no VALUE_ITEM found
                let entry_idx = entry.index();
                let new_entry_green = build_entry_green(key, &value);
                let new_entry_node = SyntaxNode::new_root_mut(new_entry_green);
                mapping.splice_children(entry_idx..entry_idx + 1, vec![new_entry_node.into()]);
            }
        } else {
            // Add new entry at the end
            let new_entry_green = build_entry_green(key, &value);
            let new_entry_node = SyntaxNode::new_root_mut(new_entry_green);
            let insert_pos = mapping.children().count();
            mapping.splice_children(insert_pos..insert_pos, vec![new_entry_node.into()]);
        }

        Ok(())
    }

    /// Remove a key from the YAML document
    pub fn remove(&self, key: &str) -> Result<Option<Value>> {
        // Update the code value (for get() to work)
        let removed_value = {
            let mut code = self.code.borrow_mut();
            if let Value::Map(ref mut map) = *code {
                map.shift_remove(key)
            } else {
                None
            }
        };

        // Update the CST if the key was found
        if removed_value.is_some() {
            let cst = self.cst.borrow_mut();

            // Find the MAPPING node in the CST
            if let Some(mapping) = cst.descendants().find(|n| n.kind() == SyntaxKind::MAPPING) {
                // Find the ENTRY node for this key within the mapping
                let entry_idx = mapping.children().position(|child| {
                    if child.kind() == SyntaxKind::ENTRY {
                        // Extract the full key (including trailing colon if double colon case)
                        let (entry_key, _) = extract_entry_pair(&child);
                        return entry_key == key;
                    }
                    false
                });

                if let Some(idx) = entry_idx {
                    // Remove the entry
                    mapping.splice_children(idx..idx + 1, vec![]);
                }
            }
        }

        Ok(removed_value)
    }

    /// Check if a key exists in the YAML document
    pub fn contains_key(&self, key: &str) -> Result<bool> {
        let code = self.code.borrow();
        if let Value::Map(ref map) = *code {
            Ok(map.contains_key(key))
        } else {
            Ok(false)
        }
    }

    /// Get all keys from the YAML document
    pub fn keys(&self) -> Result<Vec<String>> {
        let code = self.code.borrow();
        if let Value::Map(ref map) = *code {
            Ok(map.keys().cloned().collect())
        } else {
            Ok(Vec::new())
        }
    }

    /// Clear all entries from the YAML document
    pub fn clear(&self) -> Result<()> {
        *self.code.borrow_mut() = Value::Map(IndexMap::new());

        // Also clear the CST
        let cst = self.cst.borrow();
        if let Some(mapping) = cst.descendants().find(|n| n.kind() == SyntaxKind::MAPPING) {
            let num_children = mapping.children().count();
            if num_children > 0 {
                // Remove all ENTRY children
                mapping.splice_children(0..num_children, vec![]);
            }
        }

        Ok(())
    }

    /// Force a rewrite of the entire YAML file
    pub fn force_rewrite(&self) -> Result<()> {
        *self.force_rewrite_flag.borrow_mut() = true;
        Ok(())
    }

    /// Update multiple fields with custom ordering
    ///
    /// This inserts new fields in the correct position according to the provided field order.
    ///
    /// # Arguments
    /// * `changes` - Vec of (key, value) pairs to update
    /// * `field_order` - Slice of field names in desired order
    pub fn update_with_order(
        &self,
        changes: Vec<(&str, Value)>,
        field_order: &[&str],
    ) -> Result<()> {
        // Check if document is a map - if not, skip (nothing to update)
        let existing_keys = {
            let code = self.code.borrow();
            match *code {
                Value::Map(ref map) => map.keys().map(|k| k.to_string()).collect::<Vec<_>>(),
                _ => return Ok(()), // Not a map, nothing to update
            }
        };

        // Update existing fields in place (preserves their position)
        for (key, value) in &changes {
            if existing_keys.iter().any(|k| k == *key) {
                self.set(key, value.clone())?;
            }
        }

        // Insert new fields at the correct position based on field_order
        let new_fields: Vec<_> = changes
            .into_iter()
            .filter(|(k, _)| !existing_keys.iter().any(|existing| existing == *k))
            .collect();

        for (key, value) in new_fields {
            self.set_with_field_order(key, value, field_order)?;
        }

        Ok(())
    }

    /// Set a field with field ordering - finds the correct insertion position
    fn set_with_field_order(&self, key: &str, value: Value, field_order: &[&str]) -> Result<()> {
        // Update the code value
        let mut code = self.code.borrow_mut();
        if let Value::Map(ref mut map) = *code {
            map.insert(key.to_string(), value.clone());
        } else {
            return Err(Error::ValueError("Document is not a map".to_string()));
        }
        drop(code);

        // Find insertion position in CST based on field_order
        let cst = self.cst.borrow();
        let mapping = cst
            .descendants()
            .find(|n| n.kind() == SyntaxKind::MAPPING)
            .ok_or_else(|| Error::ValueError("No MAPPING node found".to_string()))?;

        let insertion_index = self.find_insertion_index(key, field_order, &mapping);

        let new_entry_green = build_entry_green(key, &value);
        let new_entry_node = SyntaxNode::new_root_mut(new_entry_green);
        mapping.splice_children(
            insertion_index..insertion_index,
            vec![new_entry_node.into()],
        );

        Ok(())
    }

    /// Find the appropriate insertion index for a new field based on field ordering
    fn find_insertion_index(&self, key: &str, field_order: &[&str], mapping: &SyntaxNode) -> usize {
        // Find position of the new field in the canonical order
        let new_field_position = field_order.iter().position(|&field| field == key);

        let mut insertion_index = mapping.children().count();

        // Find the right position based on canonical field order
        for (i, child) in mapping.children().enumerate() {
            if child.kind() == SyntaxKind::ENTRY {
                let (existing_key, _) = extract_entry_pair(&child);
                let existing_position = field_order.iter().position(|&field| field == existing_key);

                match (new_field_position, existing_position) {
                    // Both fields are in the canonical order
                    (Some(new_pos), Some(existing_pos)) => {
                        if new_pos < existing_pos {
                            insertion_index = i;
                            break;
                        }
                    }
                    // New field is in canonical order, existing is not - continue looking
                    (Some(_), None) => {}
                    // New field is not in canonical order, existing is - continue
                    (None, Some(_)) => {}
                    // Neither field is in canonical order, maintain alphabetical
                    (None, None) => {
                        if key < existing_key.as_str() {
                            insertion_index = i;
                            break;
                        }
                    }
                }
            }
        }

        insertion_index
    }

    /// Check if the loaded code is a list (not a dict)
    pub fn is_list(&self) -> Result<bool> {
        let code = self.code.borrow();
        Ok(matches!(*code, Value::List(_)))
    }

    /// Get the entire document as a Value
    pub fn get_all(&self) -> Result<Value> {
        Ok(self.code.borrow().clone())
    }

    /// Set the entire document from a Value
    pub fn set_all(&self, value: Value) -> Result<()> {
        // Update the code value
        *self.code.borrow_mut() = value.clone();

        // Rebuild the entire CST from scratch
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::ROOT));
        builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::DOCUMENT));

        // Add document marker if it was present in the original
        let orig_cst = self.cst.borrow();
        let had_doc_start = orig_cst
            .descendants_with_tokens()
            .any(|n| matches!(n.kind(), SyntaxKind::DOC_START));
        drop(orig_cst);

        if had_doc_start {
            builder.token(YamlLanguage::kind_to_raw(SyntaxKind::DOC_START), "---");
            builder.token(YamlLanguage::kind_to_raw(SyntaxKind::NEWLINE), "\n");
        }

        // Build the new structure
        match &value {
            Value::Map(map) => {
                builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::MAPPING));
                for (key, val) in map {
                    // Build each entry inline
                    builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::ENTRY));
                    // KEY_ITEM node wrapping KEY token
                    builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::KEY_ITEM));
                    builder.token(YamlLanguage::kind_to_raw(SyntaxKind::KEY), key);
                    builder.finish_node(); // KEY_ITEM
                    builder.token(YamlLanguage::kind_to_raw(SyntaxKind::COLON), ":");
                    builder.token(YamlLanguage::kind_to_raw(SyntaxKind::WHITESPACE), " ");
                    // VALUE_ITEM node with properly structured value
                    builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::VALUE_ITEM));
                    build_value_nodes(&mut builder, val, false);
                    builder.finish_node(); // VALUE_ITEM
                    builder.token(YamlLanguage::kind_to_raw(SyntaxKind::NEWLINE), "\n");
                    builder.finish_node(); // ENTRY
                }
                builder.finish_node(); // MAPPING
            }
            _ => {
                // For non-map values, we need to rebuild differently
                // For now, return an error as this is unexpected
                return Err(Error::ValueError(
                    "set_all currently only supports Map values".to_string(),
                ));
            }
        }

        builder.finish_node(); // DOCUMENT
        builder.finish_node(); // ROOT

        let new_green = builder.finish();
        let new_tree = SyntaxNode::new_root_mut(new_green);
        *self.cst.borrow_mut() = new_tree;

        Ok(())
    }
}

impl Drop for YamlDocument {
    fn drop(&mut self) {
        let _ = self.save();
    }
}

impl YamlDocument {
    fn save(&mut self) -> Result<()> {
        let code = self.code.borrow();

        if let Value::Map(ref code_map) = *code {
            if code_map.is_empty() && self.remove_empty {
                if self.path.exists() {
                    std::fs::remove_file(&self.path)?;

                    // Remove parent directory if empty
                    if let Some(parent) = self.path.parent() {
                        if parent.read_dir()?.next().is_none() {
                            let _ = std::fs::remove_dir(parent);
                        }
                    }
                }
                return Ok(());
            }
        }

        if *code != self.orig {
            // Create parent directory if needed
            if let Some(parent) = self.path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            let mut file = std::fs::File::create(&self.path)?;

            for line in &self.preamble {
                writeln!(file, "{}", line)?;
            }

            // Write earlier documents (each already contains its own --- marker and formatting)
            for earlier_cst in &self.earlier_docs_csts {
                write!(file, "{}", earlier_cst.text())?;
            }

            // Serialize the last/editable document CST - preserves all original formatting
            write!(file, "{}", self.cst.borrow().text())?;
        }

        Ok(())
    }
}

/// A multi-document YAML editor
pub struct MultiYamlUpdater {
    path: std::path::PathBuf,
    remove_empty: bool,
}

impl MultiYamlUpdater {
    /// Create a new MultiYamlUpdater for the given path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(MultiYamlUpdater {
            path: path.as_ref().to_path_buf(),
            remove_empty: false,
        })
    }

    /// Set whether to remove the file if it becomes empty
    pub fn remove_empty(mut self, remove_empty: bool) -> Self {
        self.remove_empty = remove_empty;
        self
    }

    /// Enter the context manager and load the YAML file
    pub fn open(&mut self) -> Result<MultiYamlDocument> {
        let (orig, preamble, csts) = if self.path.exists() {
            let content = std::fs::read_to_string(&self.path)?;
            let lines: Vec<&str> = content.lines().collect();

            let mut preamble = Vec::new();
            let mut i = 0;

            // Extract preamble (empty lines, comments, and YAML directives)
            while i < lines.len() {
                let line = lines[i];
                if line.trim().is_empty() || line.starts_with('#') || line.starts_with('%') {
                    preamble.push(line.to_string());
                    i += 1;
                } else {
                    break;
                }
            }

            let mut rest = lines[i..].join("\n");
            // Preserve trailing newline if the original content had one
            if content.ends_with('\n') && !rest.ends_with('\n') {
                rest.push('\n');
            }

            // Find document markers to split into separate documents
            // We look for "\n---" but want to split AFTER the newline, so each document includes its "---"
            let mut doc_start_positions: Vec<usize> = Vec::new();
            if rest.starts_with("---") {
                doc_start_positions.push(0);
            }
            for (pos, _) in rest.match_indices("\n---") {
                // pos points to the \n, so the next document starts at pos+1 (after the \n)
                doc_start_positions.push(pos + 1);
            }

            let (values, csts) = if doc_start_positions.is_empty() && rest.trim().is_empty() {
                // Empty file
                (Vec::new(), Vec::new())
            } else if doc_start_positions.is_empty() {
                // Single document without explicit marker
                let tree = parse_yaml(&rest);
                let value = extract_value_from_cst(&tree);
                (vec![value], vec![tree])
            } else {
                // Multiple documents - split and parse each
                let mut doc_texts = Vec::new();
                for i in 0..doc_start_positions.len() {
                    let start = doc_start_positions[i];
                    let end = if i + 1 < doc_start_positions.len() {
                        doc_start_positions[i + 1] - 1 // Exclude the newline before next ---
                    } else {
                        rest.len()
                    };
                    doc_texts.push(&rest[start..end]);
                }

                let mut values = Vec::new();
                let mut csts = Vec::new();
                for doc_text in doc_texts {
                    let tree = parse_yaml(doc_text);
                    let value = extract_value_from_cst(&tree);
                    values.push(value);
                    csts.push(tree);
                }
                (values, csts)
            };

            (values, preamble, csts)
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };

        let code = orig.clone();

        Ok(MultiYamlDocument {
            path: self.path.clone(),
            orig,
            code: RefCell::new(code),
            preamble,
            remove_empty: self.remove_empty,
            csts: RefCell::new(csts),
        })
    }

    /// Close the context manager and save changes
    pub fn close(&mut self) -> Result<()> {
        // In native implementation, closing is handled by Drop
        Ok(())
    }
}

/// A multi-document YAML file that can be edited
pub struct MultiYamlDocument {
    path: std::path::PathBuf,
    orig: Vec<Value>,
    code: RefCell<Vec<Value>>,
    preamble: Vec<String>,
    remove_empty: bool,
    csts: RefCell<Vec<SyntaxNode>>, // CST for each document
}

impl MultiYamlDocument {
    /// Get the number of documents
    pub fn len(&self) -> Result<usize> {
        Ok(self.code.borrow().len())
    }

    /// Check if there are no documents
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.code.borrow().is_empty())
    }

    /// Get a document by index
    pub fn get(&self, index: usize) -> Result<Option<Value>> {
        Ok(self.code.borrow().get(index).cloned())
    }

    /// Set a document at the given index
    pub fn set(&self, index: usize, value: Value) -> Result<()> {
        let mut code = self.code.borrow_mut();
        let csts = self.csts.borrow();

        if index >= code.len() || index >= csts.len() {
            return Err(Error::ValueError(format!("Index {} out of bounds", index)));
        }

        // Only support Map values for now
        let Value::Map(new_map) = &value else {
            return Err(Error::ValueError(
                "set() currently only supports Map values".to_string(),
            ));
        };

        // Update the code value
        code[index] = value.clone();
        drop(code); // Release borrow

        // Get the CST for this document
        let cst = &csts[index];

        // Find the MAPPING node in the CST
        let mapping = cst
            .descendants()
            .find(|n| n.kind() == SyntaxKind::MAPPING)
            .ok_or_else(|| Error::ValueError("No MAPPING node found".to_string()))?;

        // For each key-value in the new map, update the CST
        for (key, val) in new_map {
            // Find existing entry for this key
            let existing_entry = mapping.children().find(|child| {
                if child.kind() == SyntaxKind::ENTRY {
                    let (entry_key, _) = extract_entry_pair(child);
                    return entry_key == *key;
                }
                false
            });

            if let Some(entry) = existing_entry {
                // Update existing entry
                let entry_idx = entry.index();
                let new_entry_green = build_entry_green(key, val);
                let new_entry_node = SyntaxNode::new_root_mut(new_entry_green);
                mapping.splice_children(entry_idx..entry_idx + 1, vec![new_entry_node.into()]);
            } else {
                // Add new entry at the end
                let new_entry_green = build_entry_green(key, val);
                let new_entry_node = SyntaxNode::new_root_mut(new_entry_green);
                let insert_pos = mapping.children().count();
                mapping.splice_children(insert_pos..insert_pos, vec![new_entry_node.into()]);
            }
        }

        // Remove any keys that were in the old map but not in the new map
        let old_keys: Vec<String> = mapping
            .children()
            .filter(|child| child.kind() == SyntaxKind::ENTRY)
            .map(|entry| extract_entry_pair(&entry).0)
            .collect();

        for old_key in old_keys {
            if !new_map.contains_key(&old_key) {
                // Remove this entry from CST
                let entry_idx = mapping.children().position(|child| {
                    if child.kind() == SyntaxKind::ENTRY {
                        let (entry_key, _) = extract_entry_pair(&child);
                        return entry_key == old_key;
                    }
                    false
                });

                if let Some(idx) = entry_idx {
                    mapping.splice_children(idx..idx + 1, vec![]);
                }
            }
        }

        Ok(())
    }

    /// Append a document to the list
    pub fn append(&self, value: Value) -> Result<()> {
        self.code.borrow_mut().push(value.clone());

        // Build a CST for the new document
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::ROOT));
        builder.start_node(YamlLanguage::kind_to_raw(SyntaxKind::DOCUMENT));

        // Multi-document YAML always has --- markers
        builder.token(YamlLanguage::kind_to_raw(SyntaxKind::DOC_START), "---");
        builder.token(YamlLanguage::kind_to_raw(SyntaxKind::NEWLINE), "\n");

        // Build the document content (handles all value types)
        build_value_nodes(&mut builder, &value, false);

        builder.finish_node(); // DOCUMENT
        builder.finish_node(); // ROOT

        let green = builder.finish();
        let cst = SyntaxNode::new_root_mut(green);

        self.csts.borrow_mut().push(cst);

        Ok(())
    }

    /// Remove a document at the given index
    pub fn remove(&self, index: usize) -> Result<Value> {
        let mut code = self.code.borrow_mut();
        let mut csts = self.csts.borrow_mut();
        if index < code.len() && index < csts.len() {
            csts.remove(index);
            Ok(code.remove(index))
        } else {
            Err(Error::ValueError(format!("Index {} out of bounds", index)))
        }
    }
}

impl Drop for MultiYamlDocument {
    fn drop(&mut self) {
        let _ = self.save();
    }
}

impl MultiYamlDocument {
    fn save(&mut self) -> Result<()> {
        let code = self.code.borrow();
        let csts = self.csts.borrow();

        if code.is_empty() && self.remove_empty {
            if self.path.exists() {
                std::fs::remove_file(&self.path)?;

                if let Some(parent) = self.path.parent() {
                    if parent.read_dir()?.next().is_none() {
                        let _ = std::fs::remove_dir(parent);
                    }
                }
            }
            return Ok(());
        }

        if *code != self.orig {
            let mut file = std::fs::File::create(&self.path)?;

            // Write preamble
            for line in &self.preamble {
                writeln!(file, "{}", line)?;
            }

            // Write each document's CST (preserves all formatting)
            for cst in csts.iter() {
                write!(file, "{}", cst.text())?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_yaml_updater_get_set() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        // Create initial YAML file
        fs::write(&yaml_path, "key1: value1\nkey2: value2\n").unwrap();

        // Test reading
        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            let value = doc.get("key1").unwrap();
            assert_eq!(value, Some(Value::String("value1".to_string())));

            assert!(doc.contains_key("key1").unwrap());
            assert!(!doc.contains_key("key3").unwrap());
        }

        // Test writing
        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            doc.set("key3", Value::String("value3".to_string()))
                .unwrap();

            updater.close().unwrap();
        }

        // Verify file contents
        let content = fs::read_to_string(&yaml_path).unwrap();
        assert_eq!(content, "key1: value1\nkey2: value2\nkey3: value3\n");
    }

    #[test]
    fn test_yaml_updater_force_rewrite() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(&yaml_path, "key1: value1\n").unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            doc.force_rewrite().unwrap();

            doc.set("key2", Value::String("value2".to_string()))
                .unwrap();

            updater.close().unwrap();
        }

        // Verify file was written
        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("key2"));
    }

    #[test]
    fn test_yaml_updater_remove_empty() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(&yaml_path, "key1: value1\n").unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap().remove_empty(true);
            let doc = updater.open().unwrap();

            // Remove all keys
            doc.remove("key1").unwrap();

            // Clear the document
            doc.clear().unwrap();

            updater.close().unwrap();
        }

        // File should be deleted
        assert!(!yaml_path.exists());
    }

    #[test]
    fn test_yaml_updater_new_file() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("new.yaml");

        // File doesn't exist yet
        assert!(!yaml_path.exists());

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            doc.set("newkey", Value::String("newvalue".to_string()))
                .unwrap();

            updater.close().unwrap();
        }

        // File should now exist
        assert!(yaml_path.exists());
        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("newkey"));
    }

    #[test]
    fn test_yaml_updater_clear() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(&yaml_path, "key1: value1\n").unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // Clear all keys and set new ones
            doc.clear().unwrap();
            doc.set("replaced", Value::String("yes".to_string()))
                .unwrap();

            updater.close().unwrap();
        }

        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("replaced"));
        assert!(!content.contains("key1"));
    }

    #[test]
    fn test_multi_yaml_updater_basic() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("multi.yaml");

        // Create initial multi-doc YAML file
        fs::write(&yaml_path, "---\ndoc: 1\n---\ndoc: 2\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            assert_eq!(doc.len().unwrap(), 2);
            assert!(!doc.is_empty().unwrap());

            let first = doc.get(0).unwrap();
            assert!(first.is_some());

            let second = doc.get(1).unwrap();
            assert!(second.is_some());

            // Out of bounds
            let none = doc.get(5).unwrap();
            assert!(none.is_none());
        }
    }

    #[test]
    fn test_multi_yaml_updater_set() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("multi.yaml");

        fs::write(&yaml_path, "---\ndoc: 1\n---\ndoc: 2\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            let mut map = IndexMap::new();
            map.insert("doc".to_string(), Value::Int(999));
            doc.set(0, Value::Map(map)).unwrap();

            updater.close().unwrap();
        }

        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("999"));
    }

    #[test]
    fn test_multi_yaml_updater_append() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("multi.yaml");

        fs::write(&yaml_path, "---\ndoc: 1\n---\ndoc: 2\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            assert_eq!(doc.len().unwrap(), 2);

            let mut map = IndexMap::new();
            map.insert("doc".to_string(), Value::Int(3));
            doc.append(Value::Map(map)).unwrap();

            assert_eq!(doc.len().unwrap(), 3);

            updater.close().unwrap();
        }

        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("doc: 3"));
    }

    #[test]
    fn test_multi_yaml_updater_remove() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("multi.yaml");

        fs::write(&yaml_path, "---\ndoc: 1\n---\ndoc: 2\n---\ndoc: 3\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            assert_eq!(doc.len().unwrap(), 3);

            let removed = doc.remove(1).unwrap();
            if let Value::Map(map) = removed {
                assert_eq!(map.get("doc"), Some(&Value::Int(2)));
            } else {
                panic!("Expected Map");
            }

            assert_eq!(doc.len().unwrap(), 2);

            updater.close().unwrap();
        }

        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("doc: 1"));
        assert!(!content.contains("doc: 2"));
        assert!(content.contains("doc: 3"));
    }

    #[test]
    fn test_multi_yaml_updater_empty() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("empty.yaml");

        fs::write(&yaml_path, "").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            assert_eq!(doc.len().unwrap(), 0);
            assert!(doc.is_empty().unwrap());
        }
    }

    #[test]
    fn test_multi_yaml_updater_remove_empty() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(&yaml_path, "---\ndoc: 1\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path)
                .unwrap()
                .remove_empty(true);
            let doc = updater.open().unwrap();

            // Remove all documents
            doc.remove(0).unwrap();

            updater.close().unwrap();
        }

        // File should be deleted
        assert!(!yaml_path.exists());
    }

    #[test]
    fn test_multi_yaml_updater_set_out_of_bounds() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("multi.yaml");

        fs::write(&yaml_path, "---\ndoc: 1\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            let mut map = IndexMap::new();
            map.insert("doc".to_string(), Value::Int(999));
            let result = doc.set(999, Value::Map(map));

            assert!(result.is_err());
            match result {
                Err(Error::ValueError(msg)) => {
                    assert!(msg.contains("out of bounds"));
                }
                _ => panic!("Expected ValueError"),
            }
        }
    }

    #[test]
    fn test_multi_yaml_updater_remove_out_of_bounds() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("multi.yaml");

        fs::write(&yaml_path, "---\ndoc: 1\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            let result = doc.remove(999);

            assert!(result.is_err());
            match result {
                Err(Error::ValueError(msg)) => {
                    assert!(msg.contains("out of bounds"));
                }
                _ => panic!("Expected ValueError"),
            }
        }
    }

    #[test]
    fn test_merge_duplicate_keys_no_duplicates() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        let input = "---\nName: Test\nVersion: 1.0\nAuthor: Someone\n";
        fs::write(&yaml_path, input).unwrap();

        // Should return None when there are no duplicates
        let result = merge_duplicate_keys_in_place(&yaml_path, &["Reference", "Screenshots"])
            .expect("merge should succeed");
        assert_eq!(result, None);

        // File should be unchanged
        let output = fs::read_to_string(&yaml_path).unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn test_merge_duplicate_keys_with_duplicates_non_sequence() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(&yaml_path, "---\nName: First\nVersion: 1.0\nName: Second\n").unwrap();

        // Should merge duplicates, keeping only the first
        let keys = merge_duplicate_keys_in_place(&yaml_path, &["Reference", "Screenshots"])
            .expect("merge should succeed");
        assert_eq!(keys, Some(vec!["Name".to_string()]));

        // File should have only the first Name
        let content = fs::read_to_string(&yaml_path).unwrap();
        let expected = "---\nName: First\nVersion: 1.0\n";
        assert_eq!(content, expected);
    }

    #[test]
    fn test_merge_duplicate_keys_sequence_field() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(
            &yaml_path,
            "---\nName: Test Package\nReference:\n  Author: First Author\n  Title: First Title\nReference:\n  Author: Second Author\n  Title: Second Title\n",
        )
        .unwrap();

        // Should merge References into a list
        let keys = merge_duplicate_keys_in_place(&yaml_path, &["Reference", "Screenshots"])
            .expect("merge should succeed");
        assert_eq!(keys, Some(vec!["Reference".to_string()]));

        // Verify the complete file contents with both references merged into a list
        let actual = fs::read_to_string(&yaml_path).unwrap();
        let expected = "---\nName: Test Package\nReference:\n- Author: First Author\n  Title: First Title\n- Author: Second Author\n  Title: Second Title\n";
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_merge_duplicate_keys_preserves_string_formatting() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(
            &yaml_path,
            "---\nName: Test\nReference:\n  Title: First\n  Volume: 01\nReference:\n  Title: Second\n  Volume: 02\n",
        )
        .unwrap();

        // Should merge and preserve "01" and "02" as strings (not convert to integers)
        merge_duplicate_keys_in_place(&yaml_path, &["Reference", "Screenshots"])
            .expect("merge should succeed");

        // Verify complete file contents with string formatting preserved
        let actual = fs::read_to_string(&yaml_path).unwrap();
        let expected = "---\nName: Test\nReference:\n- Title: First\n  Volume: 01\n- Title: Second\n  Volume: 02\n";
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_merge_duplicate_keys_multiple_duplicates() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(
            &yaml_path,
            "---\nName: Test\nReference:\n  Title: First\nReference:\n  Title: Second\nReference:\n  Title: Third\n",
        )
        .unwrap();

        // Should return 2 entries (for the 2 duplicates removed, keeping the first)
        let keys = merge_duplicate_keys_in_place(&yaml_path, &["Reference", "Screenshots"])
            .expect("merge should succeed");
        assert_eq!(
            keys,
            Some(vec!["Reference".to_string(), "Reference".to_string()])
        );

        // Verify complete file contents with all three references merged into a list
        let actual = fs::read_to_string(&yaml_path).unwrap();
        let expected =
            "---\nName: Test\nReference:\n- Title: First\n- Title: Second\n- Title: Third\n";
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_merge_duplicate_keys_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("nonexistent.yaml");

        // YamlUpdater doesn't create file if it doesn't exist and there are no changes
        let result = merge_duplicate_keys_in_place(&yaml_path, &["Reference", "Screenshots"])
            .expect("merge should succeed");
        assert_eq!(result, None);

        // File should still not exist (no changes were made)
        assert!(!yaml_path.exists());
    }

    #[test]
    fn test_preserve_whitespace_in_list() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        // Create a file with a list that has specific indentation
        let input = "Registry:\n - Name: conda:conda-forge\n   Entry: r-tsne\n";
        fs::write(&yaml_path, input).unwrap();

        // Add a new field using YamlUpdater
        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();
            doc.set("Archive", Value::String("CRAN".to_string()))
                .unwrap();
            updater.close().unwrap();
        }

        // Read back
        let output = fs::read_to_string(&yaml_path).unwrap();

        // Check that the Registry list indentation is preserved
        assert!(
            output.contains(" - Name: conda:conda-forge\n"),
            "First list item should have 1 space before dash"
        );
        assert!(
            output.contains("   Entry: r-tsne\n"),
            "Continuation line should have 3 spaces"
        );
    }

    #[test]
    fn test_parse_list_at_root() {
        // Test that our parser can actually parse a list at root
        let input = "- Archive: GitHub\n- Name: Test\n";

        let tree = parse_yaml(input);
        println!("CST:\n{:#?}", tree);

        // The tree should contain SEQUENCE and DASH nodes, not just treat it as garbage
    }

    #[test]
    fn test_double_colon_in_key() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        // Create a file with double colon (malformed YAML)
        let input = "Repository:: https://github.com/example\n";
        fs::write(&yaml_path, input).unwrap();

        // Parser treats this as "Repository:" (key with colon) with value "https://github.com/example"
        // Per YAML spec: first : is part of key (not followed by space), second : is separator
        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // Key should be "Repository:" not "Repository"
            let val = doc.get("Repository:").unwrap();
            println!("Repository: value: {:?}", val);

            // Value should be the URL without leading colon
            assert_eq!(
                val,
                Some(Value::String("https://github.com/example".to_string()))
            );

            updater.close().unwrap();
        }

        // File is unchanged (we preserve the malformed YAML as-is)
        let output = fs::read_to_string(&yaml_path).unwrap();
        assert_eq!(output, input);

        // Check that the key name is "Repository:" (with the colon)
        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();
            let keys = doc.keys().unwrap();
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0], "Repository:");
            updater.close().unwrap();
        }
    }

    #[test]
    fn test_remove_and_set_double_colon_key() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        // Start with Repository:: (malformed)
        let input = "Repository:: https://github.com/jelmer/example\n";
        fs::write(&yaml_path, input).unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // Remove the malformed key "Repository:"
            let val = doc.remove("Repository:").unwrap();
            assert_eq!(
                val,
                Some(Value::String(
                    "https://github.com/jelmer/example".to_string()
                ))
            );

            // Set the correct key "Repository"
            doc.set(
                "Repository",
                Value::String("https://github.com/jelmer/example".to_string()),
            )
            .unwrap();

            updater.close().unwrap();
        }

        // Output should have correct key
        let output = fs::read_to_string(&yaml_path).unwrap();
        assert_eq!(output, "Repository: https://github.com/jelmer/example\n");
    }

    #[test]
    fn test_add_keys_around_list_with_multiline_items() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        // Input has a list with a multi-line item (map with two keys)
        let input = "Registry:\n - Name: conda:conda-forge\n   Entry: r-tsne\n";
        fs::write(&yaml_path, input).unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // Add keys before and after Registry
            doc.set("Archive", Value::String("CRAN".to_string()))
                .unwrap();
            doc.set(
                "Repository",
                Value::String("https://github.com/example/repo.git".to_string()),
            )
            .unwrap();

            updater.close().unwrap();
        }

        // Registry list structure should be preserved with correct indentation
        let output = fs::read_to_string(&yaml_path).unwrap();
        println!("Output:\n{}", output);
        assert!(output.contains("Registry:\n - Name: conda:conda-forge\n   Entry: r-tsne\n"));
    }

    #[test]
    fn test_multidocument_with_comments() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        // Create a multi-document YAML with comments between documents
        let input = "# Comment 1\n%YAML 1.1\n---\n%YAML 1.1\n---\n# Comment 2\nKey: Value\n";
        fs::write(&yaml_path, input).unwrap();

        // Use MultiYamlUpdater
        let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
        let doc = updater.open().unwrap();

        // Should preserve both YAML directives and comment
        updater.close().unwrap();

        let output = fs::read_to_string(&yaml_path).unwrap();
        println!("Output:\n{}", output);

        // Check that comment is preserved
        assert!(output.contains("# Comment"), "Should preserve comments");
        assert!(
            output.contains("%YAML 1.1"),
            "Should preserve YAML directives"
        );
    }

    #[test]
    fn test_yaml_directive_in_preamble() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        // YAML directive should be preserved in preamble, not parsed as a key
        let input = "%YAML 1.1\n---\nHomepage: https://example.com\n";
        fs::write(&yaml_path, input).unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // The YAML directive should NOT appear as a key
            assert_eq!(doc.get("%YAML 1.1").unwrap(), None);

            // Homepage should be readable
            assert_eq!(
                doc.get("Homepage").unwrap(),
                Some(Value::String("https://example.com".to_string()))
            );

            updater.close().unwrap();
        }

        // File should be unchanged
        let output = fs::read_to_string(&yaml_path).unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn test_nested_mapping_cst() {
        // Debug test to see what CST is produced
        let input = "Reference:\n  Author: Test Author\n  Title: Test Title\n";
        let tree = parse_yaml(input);

        // Print the CST structure
        fn print_tree(node: &SyntaxNode, indent: usize) {
            println!("{:indent$}{:?}", "", node.kind(), indent = indent * 2);
            for child in node.children_with_tokens() {
                match child {
                    rowan::NodeOrToken::Node(n) => print_tree(&n, indent + 1),
                    rowan::NodeOrToken::Token(t) => {
                        println!(
                            "{:indent$}Token {:?}: {:?}",
                            "",
                            t.kind(),
                            t.text(),
                            indent = (indent + 1) * 2
                        );
                    }
                }
            }
        }

        println!("\nCST structure for nested mapping:");
        print_tree(&tree, 0);

        // Extract and see what we get
        let value = extract_value_from_cst(&tree);
        println!("\nExtracted value: {:?}", value);
    }

    #[test]
    fn test_nested_mapping() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        // Test nested/indented structures
        let input = "---\nReference:\n  Author: Test Author\n  Title: Test Title\n";
        fs::write(&yaml_path, input).unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // Reference should map to a nested map (not null)
            let reference = doc.get("Reference").unwrap();
            println!("Reference value: {:?}", reference);

            // Should be a map containing Author and Title
            match reference {
                Some(Value::Map(map)) => {
                    assert_eq!(
                        map.get("Author"),
                        Some(&Value::String("Test Author".to_string()))
                    );
                    assert_eq!(
                        map.get("Title"),
                        Some(&Value::String("Test Title".to_string()))
                    );
                }
                other => panic!("Expected Reference to be a Map, got {:?}", other),
            }

            updater.close().unwrap();
        }

        // File should be unchanged
        let output = fs::read_to_string(&yaml_path).unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn test_list_with_urls_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        // Test that lists with URLs are preserved correctly on roundtrip
        let input = "Screenshots:\n  - https://example.com/screenshot1.png\n  - https://example.com/screenshot2.png\n";
        fs::write(&yaml_path, input).unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // Just open and close without changes
            updater.close().unwrap();
        }

        // File should be unchanged
        let output = fs::read_to_string(&yaml_path).unwrap();
        println!("Input:\n{}", input);
        println!("Output:\n{}", output);
        assert_eq!(output, input);
    }

    #[test]
    fn test_add_key_to_doc_with_list() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        // Test adding a new key to a document that has a list
        let input = "Name: Test\nScreenshots:\n  - https://example.com/screenshot1.png\n  - https://example.com/screenshot2.png\n";
        fs::write(&yaml_path, input).unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // Add a new key
            doc.set("Archive", Value::String("PyPI".to_string()))
                .unwrap();

            updater.close().unwrap();
        }

        // Check output
        let output = fs::read_to_string(&yaml_path).unwrap();
        let expected = "Name: Test\nScreenshots:\n  - https://example.com/screenshot1.png\n  - https://example.com/screenshot2.png\nArchive: PyPI\n";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_url_with_port_colon_in_value() {
        // Test YAML spec rule: colon not followed by space is part of the value
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        // URL with port number (colon not followed by space)
        let input = "Repository: https://github.com:8080/user/repo.git\n";
        fs::write(&yaml_path, input).unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // Should parse correctly with Repository as key
            let repo = doc.get("Repository").unwrap();
            assert_eq!(
                repo,
                Some(Value::String(
                    "https://github.com:8080/user/repo.git".to_string()
                ))
            );

            // Verify keys
            let keys = doc.keys().unwrap();
            assert_eq!(keys, vec!["Repository"]);
        }
    }

    #[test]
    fn test_parse_github_colon_url() {
        // Test the exact URL format from the failing test
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        let input = "---\nRepository: https://github.com:rehsack/MooX-Locale-Passthrough.git\n";
        fs::write(&yaml_path, input).unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // Debug: print all keys
            let keys = doc.keys().unwrap();
            eprintln!("Keys found: {:?}", keys);

            // Should have Repository as the key
            assert_eq!(keys, vec!["Repository"]);

            // Should parse the URL correctly
            let repo = doc.get("Repository").unwrap();
            assert_eq!(
                repo,
                Some(Value::String(
                    "https://github.com:rehsack/MooX-Locale-Passthrough.git".to_string()
                ))
            );
        }
    }

    #[test]
    fn test_debug_cst_structure_before_after_modification() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        let input = "---\nRepository: https://github.com:rehsack/old-repo.git\n";
        fs::write(&yaml_path, input).unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            eprintln!("\n=== ENTRY STRUCTURE BEFORE MODIFICATION ===");
            let cst = doc.cst.borrow();
            let mapping = cst
                .descendants()
                .find(|n| n.kind() == SyntaxKind::MAPPING)
                .unwrap();
            let entry = mapping
                .children()
                .find(|c| c.kind() == SyntaxKind::ENTRY)
                .unwrap();
            eprintln!("ENTRY text: {:?}", entry.text());
            eprintln!("ENTRY children count: {}", entry.children().count());
            for (i, child) in entry.children().enumerate() {
                eprintln!(
                    "  Child {}: kind={:?}, text={:?}",
                    i,
                    child.kind(),
                    child.text()
                );
            }
            eprintln!(
                "ENTRY children_with_tokens count: {}",
                entry.children_with_tokens().count()
            );
            for (i, child) in entry.children_with_tokens().enumerate() {
                match child {
                    rowan::NodeOrToken::Node(n) => eprintln!(
                        "  Token/Node {}: Node kind={:?}, text={:?}",
                        i,
                        n.kind(),
                        n.text()
                    ),
                    rowan::NodeOrToken::Token(t) => eprintln!(
                        "  Token/Node {}: Token kind={:?}, text={:?}",
                        i,
                        t.kind(),
                        t.text()
                    ),
                }
            }
            drop(cst);

            eprintln!("\n=== MODIFYING Repository value ===");
            doc.set(
                "Repository",
                Value::String("https://github.com/rehsack/new-repo.git".to_string()),
            )
            .unwrap();

            eprintln!("\n=== ENTRY STRUCTURE AFTER MODIFICATION ===");
            let cst = doc.cst.borrow();
            let mapping = cst
                .descendants()
                .find(|n| n.kind() == SyntaxKind::MAPPING)
                .unwrap();
            let entry = mapping
                .children()
                .find(|c| c.kind() == SyntaxKind::ENTRY)
                .unwrap();
            eprintln!("ENTRY text: {:?}", entry.text());
            eprintln!("ENTRY children count: {}", entry.children().count());
            for (i, child) in entry.children().enumerate() {
                eprintln!(
                    "  Child {}: kind={:?}, text={:?}",
                    i,
                    child.kind(),
                    child.text()
                );
            }
            drop(cst);

            updater.close().unwrap();
        }

        let output = fs::read_to_string(&yaml_path).unwrap();
        eprintln!("\n=== FINAL OUTPUT ===\n{}", output);
    }

    #[test]
    fn test_https_url_in_value() {
        // Test that https:// URLs work without quotes (colon not followed by space)
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        let input = "Homepage: https://example.com/path\n";
        fs::write(&yaml_path, input).unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            let homepage = doc.get("Homepage").unwrap();
            assert_eq!(
                homepage,
                Some(Value::String("https://example.com/path".to_string()))
            );
        }
    }

    #[test]
    fn test_url_modification_preserves_format() {
        // Test that modifying a field with a URL preserves the value correctly
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        let input = "---\nRepository: https://github.com:rehsack/old-repo.git\n";
        fs::write(&yaml_path, input).unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // Verify we can read the URL correctly
            let repo = doc.get("Repository").unwrap();
            assert_eq!(
                repo,
                Some(Value::String(
                    "https://github.com:rehsack/old-repo.git".to_string()
                ))
            );

            // Modify to a new URL
            doc.set(
                "Repository",
                Value::String("https://github.com/rehsack/new-repo.git".to_string()),
            )
            .unwrap();

            updater.close().unwrap();
        }

        // Check output
        let output = fs::read_to_string(&yaml_path).unwrap();
        let expected = "---\nRepository: https://github.com/rehsack/new-repo.git\n";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_git_ssh_url_with_colon() {
        // Test values with multiple colons (like git@github.com:user/repo.git)
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        let input = "Repository: git@github.com:user/repo.git\n";
        fs::write(&yaml_path, input).unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            let repo = doc.get("Repository").unwrap();
            assert_eq!(
                repo,
                Some(Value::String("git@github.com:user/repo.git".to_string()))
            );
        }
    }

    #[test]
    fn test_parse_json_format() {
        let temp_dir = TempDir::new().unwrap();
        let json_path = temp_dir.path().join("test.json");

        let json_content = r#"{
  "Name": "yep",
  "Repository": [
    "url1",
    "url2"
  ]
}"#;
        fs::write(&json_path, json_content).unwrap();

        let mut updater = YamlUpdater::new(&json_path).unwrap();
        let doc = updater.open().unwrap();

        // Check if we can read Name
        let name = doc.get("Name").unwrap();
        assert_eq!(name, Some(Value::String("yep".to_string())));

        // Check if we can read Repository as a list
        let repo = doc.get("Repository").unwrap();
        assert_eq!(
            repo,
            Some(Value::List(vec![
                Value::String("url1".to_string()),
                Value::String("url2".to_string())
            ]))
        );
    }

    #[test]
    fn test_modify_json_format() {
        let temp_dir = TempDir::new().unwrap();
        let json_path = temp_dir.path().join("test.json");

        let json_content = r#"{
  "Name": "yep",
  "Repo": "old"
}
"#;
        fs::write(&json_path, json_content).unwrap();

        {
            let mut updater = YamlUpdater::new(&json_path).unwrap();
            let doc = updater.open().unwrap();

            // Modify Repo value
            doc.set("Repo", Value::String("new".to_string())).unwrap();
        }

        // Read back and verify exact content
        let content = fs::read_to_string(&json_path).unwrap();
        let expected = r#"{
  "Name": "yep",
  "Repo": "new"
}
"#;
        assert_eq!(content, expected);
    }
}
