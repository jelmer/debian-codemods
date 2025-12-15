#![deny(missing_docs)]
#![allow(clippy::type_complexity)]
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))]

//! A lossless systemd unit file parser and editor.
//!
//! This library provides a lossless parser for systemd unit files as specified
//! by the [systemd.syntax(7)](https://www.freedesktop.org/software/systemd/man/latest/systemd.syntax.html)
//! and [systemd.unit(5)](https://www.freedesktop.org/software/systemd/man/latest/systemd.unit.html).
//! It preserves all whitespace, comments, and formatting.
//! It is based on the [rowan] library.

mod lex;
mod parse;
mod unit;

/// Drop-in directory support
mod dropin;

/// Systemd specifier expansion
pub mod specifier;

/// Systemd time span parsing
pub mod timespan;

/// Systemd-specific metadata and domain knowledge
pub mod systemd_metadata;

pub use lex::SyntaxKind;
pub use parse::Parse;
pub use rowan::TextRange;
pub use specifier::SpecifierContext;
pub use timespan::{parse_timespan, TimespanParseError};
pub use unit::{Entry, Error, Lang, ParseError, PositionedParseError, Section, SystemdUnit};
