#![deny(missing_docs)]
#![allow(clippy::type_complexity)]
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))]

//! A lossless .desktop file parser and editor.
//!
//! This library provides a lossless parser for .desktop files as specified
//! by the [freedesktop.org Desktop Entry Specification](https://specifications.freedesktop.org/desktop-entry-spec/latest/).
//! It preserves all whitespace, comments, and formatting.
//! It is based on the [rowan] library.

mod desktop;
mod lex;
mod parse;

pub use desktop::{Desktop, Entry, Error, Group, Lang, ParseError, PositionedParseError};
pub use lex::SyntaxKind;
pub use parse::Parse;
pub use rowan::TextRange;
