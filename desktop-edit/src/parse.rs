//! Parse wrapper type following rust-analyzer's pattern for thread-safe storage in Salsa.

use crate::desktop::{Desktop, ParseError, PositionedParseError};
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
        T: AstNode<Language = crate::desktop::Lang>,
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
        T: AstNode<Language = crate::desktop::Lang>,
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
    pub fn syntax_node(&self) -> SyntaxNode<crate::desktop::Lang> {
        SyntaxNode::new_root_mut(self.green.clone())
    }
}

// Implement Send + Sync since GreenNode is thread-safe
unsafe impl<T> Send for Parse<T> {}
unsafe impl<T> Sync for Parse<T> {}

impl Parse<Desktop> {
    /// Parse INI/.desktop text, returning a Parse result
    pub fn parse_desktop(text: &str) -> Self {
        let parsed = crate::desktop::parse(text);
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
        let input = "[Desktop Entry]\nName=Example\n";
        let parsed = Parse::<Desktop>::parse_desktop(input);

        assert!(parsed.ok());
        assert!(parsed.errors().is_empty());

        let desktop = parsed.tree();
        assert_eq!(desktop.groups().count(), 1);
    }

    #[test]
    fn test_parse_with_errors() {
        let input = "Invalid line without section\n[Desktop Entry]\n";
        let parsed = Parse::<Desktop>::parse_desktop(input);

        assert!(!parsed.ok());
        assert!(!parsed.errors().is_empty());
    }
}
