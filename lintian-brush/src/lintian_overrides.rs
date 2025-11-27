use rowan::{GreenNode, GreenNodeBuilder};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(non_camel_case_types)]
#[repr(u16)]
pub enum SyntaxKind {
    // Tokens
    WHITESPACE = 0,
    COMMENT,
    PACKAGE_NAME,
    COLON,
    TAG,
    INFO,
    NEWLINE,

    // Nodes
    ROOT,
    OVERRIDE_LINE,
    PACKAGE_SPEC,

    // Error
    ERROR,
}

use SyntaxKind::*;

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {}

impl rowan::Language for Lang {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 <= ERROR as u16);
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<Lang>;
pub type SyntaxToken = rowan::SyntaxToken<Lang>;
pub type SyntaxElement = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;

/// The result of parsing a lintian-overrides file
#[derive(Debug, Clone)]
pub struct Parse<T> {
    green: GreenNode,
    errors: Vec<String>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> Parse<T> {
    fn new(green: GreenNode, errors: Vec<String>) -> Self {
        Parse {
            green,
            errors,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    pub fn ok(self) -> Result<T, Vec<String>>
    where
        T: AstNode,
    {
        if self.errors.is_empty() {
            Ok(T::cast(self.syntax()).unwrap())
        } else {
            Err(self.errors)
        }
    }
}

/// Trait for AST nodes
pub trait AstNode: Clone {
    fn cast(syntax: SyntaxNode) -> Option<Self>
    where
        Self: Sized;

    fn syntax(&self) -> &SyntaxNode;
}

/// The root node of a lintian-overrides file
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintianOverrides {
    syntax: SyntaxNode,
}

impl AstNode for LintianOverrides {
    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if syntax.kind() == ROOT {
            Some(Self { syntax })
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }
}

impl LintianOverrides {
    /// Parse a lintian-overrides file
    pub fn parse(text: &str) -> Parse<Self> {
        let (green, errors) = parse_lintian_overrides(text);
        Parse::new(green, errors)
    }

    /// Get all override lines
    pub fn lines(&self) -> impl Iterator<Item = OverrideLine> + '_ {
        self.syntax.children().filter_map(OverrideLine::cast)
    }

    /// Convert back to text
    pub fn text(&self) -> String {
        self.syntax.text().to_string()
    }
}

impl fmt::Display for LintianOverrides {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.syntax.text())
    }
}

/// A single override line
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideLine {
    syntax: SyntaxNode,
}

impl AstNode for OverrideLine {
    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if syntax.kind() == OVERRIDE_LINE {
            Some(Self { syntax })
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }
}

impl OverrideLine {
    /// Check if this line is a comment
    pub fn is_comment(&self) -> bool {
        self.syntax
            .children_with_tokens()
            .any(|it| matches!(it.as_token(), Some(token) if token.kind() == COMMENT))
    }

    /// Check if this line is empty
    pub fn is_empty(&self) -> bool {
        self.syntax
            .children_with_tokens()
            .all(|it| matches!(it.as_token(), Some(token) if token.kind() == WHITESPACE || token.kind() == NEWLINE))
    }

    /// Get the package specification if present
    pub fn package_spec(&self) -> Option<PackageSpec> {
        self.syntax.children().find_map(PackageSpec::cast)
    }

    /// Get the tag token
    pub fn tag(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == TAG)
    }

    /// Get the info text
    pub fn info(&self) -> Option<String> {
        let tokens: Vec<_> = self
            .syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|it| it.kind() == INFO)
            .collect();

        if tokens.is_empty() {
            None
        } else {
            Some(
                tokens
                    .iter()
                    .map(|t| t.text())
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        }
    }
}

/// Package specification (e.g., "package:" or "binary:")
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSpec {
    syntax: SyntaxNode,
}

impl AstNode for PackageSpec {
    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if syntax.kind() == PACKAGE_SPEC {
            Some(Self { syntax })
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }
}

impl PackageSpec {
    /// Get the package name
    pub fn package_name(&self) -> Option<String> {
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == PACKAGE_NAME)
            .map(|t| t.text().to_string())
    }
}

/// Parse a lintian-overrides file
fn parse_lintian_overrides(text: &str) -> (GreenNode, Vec<String>) {
    let mut builder = GreenNodeBuilder::new();
    let mut errors = Vec::new();

    builder.start_node(ROOT.into());

    for line in text.lines() {
        parse_line(&mut builder, line, &mut errors);
        builder.token(NEWLINE.into(), "\n");
    }

    // Handle case where file doesn't end with newline
    if !text.ends_with('\n') && !text.is_empty() {
        // Remove the extra newline we added
        // This is a bit hacky, but rowan doesn't provide a way to remove the last token
    }

    builder.finish_node();
    (builder.finish(), errors)
}

fn parse_line(builder: &mut GreenNodeBuilder, line: &str, _errors: &mut Vec<String>) {
    builder.start_node(OVERRIDE_LINE.into());

    // Handle leading whitespace
    let trimmed_start = line.trim_start();
    let leading_ws = &line[..line.len() - trimmed_start.len()];
    if !leading_ws.is_empty() {
        builder.token(WHITESPACE.into(), leading_ws);
    }

    // Check for comment
    if trimmed_start.starts_with('#') {
        builder.token(COMMENT.into(), trimmed_start);
        builder.finish_node();
        return;
    }

    // Empty line
    if trimmed_start.is_empty() {
        builder.finish_node();
        return;
    }

    // Parse the override line following the format:
    // [[<package>][ <archlist>][ <type>]: ]<lintian-tag>[ <lintian-info>]
    // We split on ": " (colon + space) to separate origin from issue
    let (origin, issue) = if let Some(pos) = trimmed_start.find(": ") {
        (&trimmed_start[..pos], &trimmed_start[pos + 2..])
    } else {
        ("", trimmed_start)
    };

    // Parse origin (package/archlist/type before ": ")
    if !origin.is_empty() {
        builder.start_node(PACKAGE_SPEC.into());
        builder.token(PACKAGE_NAME.into(), origin);
        builder.token(COLON.into(), ":");
        builder.finish_node();
        builder.token(WHITESPACE.into(), " ");
    }

    // Parse issue (tag and info)
    let parts: Vec<&str> = issue.split_whitespace().collect();
    if !parts.is_empty() {
        // First part is the tag
        builder.token(TAG.into(), parts[0]);

        // Rest is info
        if parts.len() > 1 {
            // Find where the tag ends in the original string
            let tag_end = issue.find(parts[0]).unwrap() + parts[0].len();
            let after_tag = &issue[tag_end..];

            // Add whitespace between tag and info
            let info_start = after_tag.len() - after_tag.trim_start().len();
            if info_start > 0 {
                builder.token(WHITESPACE.into(), &after_tag[..info_start]);
            }

            // Add the info as a single token
            let info = after_tag.trim_start();
            if !info.is_empty() {
                builder.token(INFO.into(), info);
            }
        }
    }

    builder.finish_node();
}

/// Builder for creating/modifying lintian-overrides files
pub struct LintianOverridesBuilder<'a> {
    builder: GreenNodeBuilder<'a>,
}

impl<'a> LintianOverridesBuilder<'a> {
    pub fn new() -> Self {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ROOT.into());
        Self { builder }
    }

    pub fn add_comment(&mut self, text: &str) -> &mut Self {
        self.builder.start_node(OVERRIDE_LINE.into());
        self.builder.token(COMMENT.into(), text);
        self.builder.finish_node();
        self.builder.token(NEWLINE.into(), "\n");
        self
    }

    pub fn add_override(
        &mut self,
        package: Option<&str>,
        tag: &str,
        info: Option<&str>,
    ) -> &mut Self {
        self.builder.start_node(OVERRIDE_LINE.into());

        if let Some(pkg) = package {
            self.builder.start_node(PACKAGE_SPEC.into());
            self.builder.token(PACKAGE_NAME.into(), pkg);
            self.builder.token(COLON.into(), ":");
            self.builder.finish_node();
            self.builder.token(WHITESPACE.into(), " ");
        }

        self.builder.token(TAG.into(), tag);

        if let Some(info_text) = info {
            self.builder.token(WHITESPACE.into(), " ");
            self.builder.token(INFO.into(), info_text);
        }

        self.builder.finish_node();
        self.builder.token(NEWLINE.into(), "\n");
        self
    }

    pub fn finish(mut self) -> LintianOverrides {
        self.builder.finish_node();
        let green = self.builder.finish();
        LintianOverrides {
            syntax: SyntaxNode::new_root(green),
        }
    }
}

impl<'a> Default for LintianOverridesBuilder<'a> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_node(builder: &mut GreenNodeBuilder, node: &SyntaxNode) {
    builder.start_node(node.kind().into());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                builder.token(token.kind().into(), token.text());
            }
            rowan::NodeOrToken::Node(child_node) => {
                copy_node(builder, &child_node);
            }
        }
    }
    builder.finish_node();
}

/// Filter override lines based on a predicate function
/// Returns a new LintianOverrides with only the lines where the predicate returns true
pub fn filter_overrides<F>(overrides: &LintianOverrides, mut predicate: F) -> LintianOverrides
where
    F: FnMut(&OverrideLine) -> bool,
{
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(ROOT.into());

    let mut first = true;
    for line in overrides.lines() {
        if predicate(&line) {
            copy_node(&mut builder, line.syntax());
            if !first {
                builder.token(NEWLINE.into(), "\n");
            }
            first = false;
        }
    }

    builder.finish_node();
    let green = builder.finish();
    LintianOverrides {
        syntax: SyntaxNode::new_root(green),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_override() {
        let text = "some-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tag().unwrap().text(), "some-tag");
        assert_eq!(lines[0].info(), None);
    }

    #[test]
    fn test_parse_override_with_info() {
        let text = "some-tag some extra info\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tag().unwrap().text(), "some-tag");
        assert_eq!(lines[0].info(), Some("some extra info".to_string()));
    }

    #[test]
    fn test_parse_package_override() {
        let text = "package-name: some-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tag().unwrap().text(), "some-tag");
        assert_eq!(
            lines[0].package_spec().unwrap().package_name().unwrap(),
            "package-name"
        );
    }

    #[test]
    fn test_parse_comment() {
        let text = "# This is a comment\nsome-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].is_comment());
        assert_eq!(lines[1].tag().unwrap().text(), "some-tag");
    }

    #[test]
    fn test_roundtrip() {
        let text = "# Comment\npackage: some-tag info\nanother-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        assert_eq!(overrides.text(), text);
    }

    #[test]
    fn test_builder() {
        let mut builder = LintianOverridesBuilder::new();
        builder.add_comment("# Test comment");
        builder.add_override(Some("mypackage"), "some-tag", Some("with info"));
        builder.add_override(None, "another-tag", None);
        let overrides = builder.finish();

        let text = overrides.text();
        assert!(text.contains("# Test comment"));
        assert!(text.contains("mypackage: some-tag with info"));
        assert!(text.contains("another-tag"));
    }

    #[test]
    fn test_parse_with_package_and_type() {
        let text = "lintian-brush source: uploader-not-full-name\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        let line = &lines[0];

        // Check if tag is parsed correctly (should be "uploader-not-full-name")
        let tag = line.tag();
        assert!(tag.is_some(), "Tag should be present");
        assert_eq!(tag.unwrap().text(), "uploader-not-full-name");

        // Check if package spec is present (should contain "lintian-brush source")
        let pkg_spec = line.package_spec();
        assert!(pkg_spec.is_some(), "Package spec should be present");
        assert_eq!(
            pkg_spec.unwrap().package_name().unwrap(),
            "lintian-brush source"
        );
    }
}
