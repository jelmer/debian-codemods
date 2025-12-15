//! Parse wrapper type following rust-analyzer's pattern for thread-safe storage in Salsa.

use crate::unit::{ParseError, PositionedParseError, SystemdUnit};
use rowan::ast::AstNode;
use rowan::{GreenNode, SyntaxNode};
use std::marker::PhantomData;

/// The result of parsing: a syntax tree and a collection of errors.
///
/// This type is designed to be stored in Salsa databases as it contains
/// the thread-safe `GreenNode` instead of the non-thread-safe `SyntaxNode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse<T> {
    green: GreenNode,
    errors: Vec<String>,
    positioned_errors: Vec<PositionedParseError>,
    _ty: PhantomData<T>,
}

impl<T> Parse<T> {
    /// Create a new Parse result from a GreenNode and errors
    pub fn new(green: GreenNode, errors: Vec<String>) -> Self {
        Parse {
            green,
            errors,
            positioned_errors: Vec::new(),
            _ty: PhantomData,
        }
    }

    /// Create a new Parse result from a GreenNode, errors, and positioned errors
    pub fn new_with_positioned_errors(
        green: GreenNode,
        errors: Vec<String>,
        positioned_errors: Vec<PositionedParseError>,
    ) -> Self {
        Parse {
            green,
            errors,
            positioned_errors,
            _ty: PhantomData,
        }
    }

    /// Get the green node (thread-safe representation)
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// Get the syntax errors
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Get parse errors with position information
    pub fn positioned_errors(&self) -> &[PositionedParseError] {
        &self.positioned_errors
    }

    /// Get parse errors as strings (for backward compatibility if needed)
    pub fn error_messages(&self) -> Vec<String> {
        self.positioned_errors
            .iter()
            .map(|e| e.message.clone())
            .collect()
    }

    /// Check if there are any errors
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Convert to a Result, returning the tree if there are no errors
    pub fn to_result(self) -> Result<T, ParseError>
    where
        T: AstNode<Language = crate::unit::Lang>,
    {
        if self.errors.is_empty() {
            let node = SyntaxNode::new_root_mut(self.green);
            Ok(T::cast(node).expect("root node has wrong type"))
        } else {
            Err(ParseError(self.errors))
        }
    }

    /// Get the parsed syntax tree, panicking if there are errors
    pub fn tree(&self) -> T
    where
        T: AstNode<Language = crate::unit::Lang>,
    {
        assert!(
            self.errors.is_empty(),
            "tried to get tree with errors: {:?}",
            self.errors
        );
        let node = SyntaxNode::new_root_mut(self.green.clone());
        T::cast(node).expect("root node has wrong type")
    }

    /// Get the syntax node
    pub fn syntax_node(&self) -> SyntaxNode<crate::unit::Lang> {
        SyntaxNode::new_root_mut(self.green.clone())
    }
}

// Implement Send + Sync since GreenNode is thread-safe
unsafe impl<T> Send for Parse<T> {}
unsafe impl<T> Sync for Parse<T> {}

impl Parse<SystemdUnit> {
    /// Parse systemd unit text, returning a Parse result
    pub fn parse_unit(text: &str) -> Self {
        let parsed = crate::unit::parse(text);
        Parse::new_with_positioned_errors(
            parsed.green_node,
            parsed.errors,
            parsed.positioned_errors,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_success() {
        let input = "[Unit]\nDescription=Test\n";
        let parsed = Parse::<SystemdUnit>::parse_unit(input);

        assert!(parsed.ok());
        assert!(parsed.errors().is_empty());

        let unit = parsed.tree();
        assert_eq!(unit.sections().count(), 1);
    }

    #[test]
    fn test_parse_with_errors() {
        let input = "Invalid line without section\n[Unit]\n";
        let parsed = Parse::<SystemdUnit>::parse_unit(input);

        assert!(!parsed.ok());
        assert!(!parsed.errors().is_empty());
    }
}
