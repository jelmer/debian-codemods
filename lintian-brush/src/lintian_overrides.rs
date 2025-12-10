use rowan::{GreenNode, GreenNodeBuilder};
use std::fmt;

/// Check if an info string matches a pattern with wildcards
///
/// Supports `*` wildcards in patterns like `[debian/copyright:*]` matching `[debian/copyright:31]`
/// The asterisk matches arbitrary strings similar to shell wildcards.
fn info_matches(pattern: &str, value: &str) -> bool {
    if pattern == value {
        return true;
    }

    // Check if pattern contains wildcards
    if !pattern.contains('*') {
        return false;
    }

    // Split pattern by wildcards
    let parts: Vec<&str> = pattern.split('*').collect();

    // Check prefix (before first *)
    if !parts[0].is_empty() && !value.starts_with(parts[0]) {
        return false;
    }

    // Check suffix (after last *)
    if !parts[parts.len() - 1].is_empty() && !value.ends_with(parts[parts.len() - 1]) {
        return false;
    }

    // Check middle parts appear in order
    let mut pos = parts[0].len();
    for part in &parts[1..parts.len() - 1] {
        if part.is_empty() {
            continue;
        }
        if let Some(found) = value[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }

    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(non_camel_case_types)]
#[repr(u16)]
pub enum SyntaxKind {
    /// Whitespace token
    WHITESPACE = 0,
    /// Comment token
    COMMENT,
    /// Package name token
    PACKAGE_NAME,
    /// Colon token
    COLON,
    /// Tag token
    TAG,
    /// Info token
    INFO,
    /// Newline token
    NEWLINE,

    /// Root node
    ROOT,
    /// Override line node
    OVERRIDE_LINE,
    /// Package specification node
    PACKAGE_SPEC,

    /// Error node
    ERROR,
}

use SyntaxKind::*;

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

/// Language type for the lintian override parser
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

/// Syntax node type for lintian overrides
pub type SyntaxNode = rowan::SyntaxNode<Lang>;
/// Syntax token type for lintian overrides
pub type SyntaxToken = rowan::SyntaxToken<Lang>;
/// Syntax element type for lintian overrides
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

    /// Get the syntax tree
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// Get the parse errors
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Convert to result, returning errors if any
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
    /// Cast a syntax node to this AST node type
    fn cast(syntax: SyntaxNode) -> Option<Self>
    where
        Self: Sized;

    /// Get the underlying syntax node
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

    /// Get the text representation of this line
    pub fn text(&self) -> String {
        self.syntax.text().to_string()
    }

    /// Get the package type from the package spec (e.g., "source", "binary")
    /// The package spec can be in format "package-name type:" or just "type:"
    pub fn package_type(&self) -> Option<String> {
        let pkg_name = self.package_spec()?.package_name()?;
        // The package_name might be "blah source" or just "source"
        // Split on whitespace and take the last word as the type
        let parts: Vec<&str> = pkg_name.split_whitespace().collect();
        parts.last().map(|s| s.to_string())
    }

    /// Check if this override matches a LintianIssue
    pub fn matches(&self, issue: &crate::LintianIssue) -> bool {
        // Check if tag matches
        if let Some(tag) = self.tag() {
            let tag_text = tag.text();
            let issue_tag = issue.tag.as_deref();
            if Some(tag_text) != issue_tag {
                return false;
            }
        } else {
            return false;
        }

        // Check package name and/or type if specified in override
        if let Some(pkg_spec) = self.package_spec() {
            if let Some(pkg_name) = pkg_spec.package_name() {
                // Parse the package spec - could be "package-name", "binary", "source",
                // "package-name binary", or "package-name source"
                let parts: Vec<&str> = pkg_name.split_whitespace().collect();

                // If it's just "binary" or "source", match on type only
                if parts.len() == 1 && (parts[0] == "binary" || parts[0] == "source") {
                    let issue_type = issue.package_type.as_ref().map(|t| t.to_string());
                    if Some(parts[0]) != issue_type.as_deref() {
                        return false;
                    }
                } else if parts.len() == 2 && (parts[1] == "binary" || parts[1] == "source") {
                    // Format: "package-name binary" or "package-name source"
                    let issue_pkg = issue.package.as_deref();
                    let issue_type = issue.package_type.as_ref().map(|t| t.to_string());
                    if Some(parts[0]) != issue_pkg || Some(parts[1]) != issue_type.as_deref() {
                        return false;
                    }
                } else {
                    // Just a package name without explicit type - match on package name only
                    let issue_pkg = issue.package.as_deref();
                    if Some(parts[0]) != issue_pkg {
                        return false;
                    }
                }
            }
        }

        // Check info if we have it
        if let Some(ref our_info) = issue.info {
            if let Some(override_info) = self.info() {
                // Compare info - support wildcard matching
                let override_info = override_info.trim();
                let our_info_str = our_info.join(" ");
                if !info_matches(override_info, &our_info_str) {
                    return false;
                }
            }
        }

        true
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

    // Parse the override line
    let mut current_start = 0;

    // First, check if we have a package spec by looking for a colon
    // The package spec format is "package-name:" or "package-name type:"
    // We need to distinguish this from info that may contain colons (e.g., "line 51:")
    // A package spec will have:
    // 1. A colon followed by whitespace or end-of-line
    // 2. The part before the colon should be a reasonable package spec (1-2 words)
    let mut has_package_spec = false;
    let mut colon_pos = 0;

    if let Some(pos) = trimmed_start.find(':') {
        // Check if the colon is followed by whitespace or is at the end
        let after_colon = &trimmed_start[pos + 1..];
        if after_colon.is_empty() || after_colon.starts_with(char::is_whitespace) {
            // Check if the part before the colon looks like a package spec
            // It should be 1-2 words (package name, optionally with "source" or "binary")
            let before_colon = &trimmed_start[..pos];
            let words_before: Vec<&str> = before_colon.split_whitespace().collect();

            // Valid package specs:
            // - Single word: "source:", "binary:", "package-name:"
            // - Two words: "source package-name:", "binary package-name:", "package-name source:", "package-name binary:"
            let is_valid_package_spec = match words_before.len() {
                1 => true, // Single word is always valid
                2 => {
                    // Two words: either first or second must be "source" or "binary"
                    words_before[0] == "source"
                        || words_before[0] == "binary"
                        || words_before[1] == "source"
                        || words_before[1] == "binary"
                }
                _ => false, // More than 2 words is never a valid package spec
            };

            if is_valid_package_spec {
                // This looks like a valid package spec
                has_package_spec = true;
                colon_pos = pos;
            }
        }
    }

    if has_package_spec {
        // Found package spec - everything before colon is the package spec
        builder.start_node(PACKAGE_SPEC.into());
        builder.token(
            PACKAGE_NAME.into(),
            &trimmed_start[current_start..colon_pos],
        );
        builder.token(COLON.into(), ":");
        builder.finish_node();

        current_start = colon_pos + 1;

        // Skip any whitespace after colon
        let after_colon = &trimmed_start[current_start..];
        let trimmed_after = after_colon.trim_start();
        let ws_len = after_colon.len() - trimmed_after.len();
        if ws_len > 0 {
            builder.token(WHITESPACE.into(), &after_colon[..ws_len]);
            current_start += ws_len;
        }
    }

    // Now parse the rest as tag and info
    let rest = &trimmed_start[current_start..];
    let parts: Vec<&str> = rest.split_whitespace().collect();

    if !parts.is_empty() {
        // First part is the tag
        builder.token(TAG.into(), parts[0]);

        // Rest is info
        if parts.len() > 1 {
            // Find where the tag ends in the original string
            let tag_end = rest.find(parts[0]).unwrap() + parts[0].len();
            let after_tag = &rest[tag_end..];

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
    /// Create a new builder
    pub fn new() -> Self {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ROOT.into());
        Self { builder }
    }

    /// Add a comment line
    pub fn add_comment(&mut self, text: &str) -> &mut Self {
        self.builder.start_node(OVERRIDE_LINE.into());
        self.builder.token(COMMENT.into(), text);
        self.builder.finish_node();
        self.builder.token(NEWLINE.into(), "\n");
        self
    }

    /// Add an override line
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

    /// Finish building and return the LintianOverrides
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

/// Copy a syntax node into a green node builder
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

/// Find all lintian-overrides files in a debian directory
pub fn find_override_files(base_path: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    // Check debian/source/lintian-overrides
    let source_overrides = base_path.join("debian/source/lintian-overrides");
    if source_overrides.exists() {
        files.push(source_overrides);
    }

    // Check debian/*.lintian-overrides
    let debian_dir = base_path.join("debian");
    if debian_dir.exists() && debian_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&debian_dir) {
            for entry in entries.flatten() {
                if let Some(filename) = entry.file_name().to_str() {
                    if filename.ends_with(".lintian-overrides") {
                        files.push(entry.path());
                    }
                }
            }
        }
    }

    files
}

/// Iterate over all lintian override lines in a debian directory
pub fn iter_overrides(base_path: &std::path::Path) -> impl Iterator<Item = OverrideLine> {
    let files = find_override_files(base_path);

    files
        .into_iter()
        .flat_map(|override_file| {
            let content = std::fs::read_to_string(&override_file).ok()?;
            let parsed = LintianOverrides::parse(&content);
            let overrides = parsed.ok().ok()?;
            Some(overrides.lines().collect::<Vec<_>>())
        })
        .flatten()
}

/// Filter override lines based on a predicate
pub fn filter_overrides<F>(overrides: &LintianOverrides, mut predicate: F) -> LintianOverrides
where
    F: FnMut(&OverrideLine) -> bool,
{
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(ROOT.into());

    for line_node in overrides.syntax.children() {
        if line_node.kind() == OVERRIDE_LINE {
            let line = OverrideLine {
                syntax: line_node.clone(),
            };

            if predicate(&line) {
                copy_node(&mut builder, &line_node);
            }
        }
    }

    builder.finish_node();
    let green = builder.finish();
    LintianOverrides {
        syntax: SyntaxNode::new_root(green),
    }
}

/// Map override lines using a transformation function
/// Returns a new LintianOverrides with the lines transformed by the function
/// If the function returns None, the original line is kept unchanged
pub fn map_overrides<F>(overrides: &LintianOverrides, mut transform: F) -> LintianOverrides
where
    F: FnMut(&OverrideLine) -> Option<(Option<String>, String, Option<String>)>,
{
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(ROOT.into());

    for line in overrides.lines() {
        // Try to transform the line
        if let Some((package, tag, info)) = transform(&line) {
            // Build a new override line with the transformed values
            builder.start_node(OVERRIDE_LINE.into());

            if let Some(pkg) = package {
                builder.start_node(PACKAGE_SPEC.into());
                builder.token(PACKAGE_NAME.into(), &pkg);
                builder.token(COLON.into(), ":");
                builder.finish_node();
                builder.token(WHITESPACE.into(), " ");
            }

            builder.token(TAG.into(), &tag);

            if let Some(info_text) = info {
                builder.token(WHITESPACE.into(), " ");
                builder.token(INFO.into(), &info_text);
            }

            builder.finish_node();
        } else {
            // Keep the original line unchanged
            copy_node(&mut builder, line.syntax());
        }
        builder.token(NEWLINE.into(), "\n");
    }

    builder.finish_node();
    let green = builder.finish();
    LintianOverrides {
        syntax: SyntaxNode::new_root(green),
    }
}

/// Fix override info format by applying tag-specific transformations
/// Converts old format like "file (line 123)" to new format like "[file:123]"
pub fn fix_override_info(tag: &str, info: &str) -> String {
    use lazy_static::lazy_static;
    use regex::Regex;

    lazy_static! {
        // Common regex patterns - note: Rust regex doesn't support lookahead, so we match anything not [ or space
        static ref PATH_MATCH: &'static str = r"(?P<path>[^\[\s]+)";
        static ref LINENO_MATCH: &'static str = r"(?P<lineno>\d+|\*)";

        // Pure file:lineno transformations
        static ref PURE_FLN_RE: Regex = Regex::new(&format!(r"^{} \(line {}\)$", *PATH_MATCH, *LINENO_MATCH)).unwrap();
        static ref PURE_FLN_WILDCARD_RE: Regex = Regex::new(&format!(r"^{} \(line {}\)$", *PATH_MATCH, *LINENO_MATCH)).unwrap();
        static ref PURE_FN_RE: Regex = Regex::new(&format!(r"^{}$", *PATH_MATCH)).unwrap();

        // Debian rules specific
        static ref RULES_LINENO_RE: Regex = Regex::new(&format!(r"(.*) \(line {}\)", *LINENO_MATCH)).unwrap();

        // Debian source options
        static ref SOURCE_OPTIONS_RE: Regex = Regex::new(&format!(r"(.*) \(line {}\)", *LINENO_MATCH)).unwrap();

        // Copyright file patterns
        static ref COPYRIGHT_LINE_RE: Regex = Regex::new(&format!(r"^debian/copyright (.+) \(line {}\)", *LINENO_MATCH)).unwrap();
        static ref COPYRIGHT_WILDCARD_RE: Regex = Regex::new(r"^debian/copyright (.+) \*").unwrap();
        static ref COPYRIGHT_STAR_RE: Regex = Regex::new(r"^debian/copyright \*").unwrap();
        static ref COPYRIGHT_SIMPLE_RE: Regex = Regex::new(r"^([^/ ]+) \*").unwrap();

        // Permission-related
        static ref NON_STANDARD_PERM_RE: Regex = Regex::new(&format!(r"^{} ([0-9]+) != ([0-9]+)", *PATH_MATCH)).unwrap();
        static ref EXECUTABLE_PERM_RE: Regex = Regex::new(&format!(r"^{} ([0-9]+)", *PATH_MATCH)).unwrap();
        static ref SETUID_RE: Regex = Regex::new(&format!(r"^{} (?P<mode>[0-9]+) (.+/.+)", *PATH_MATCH)).unwrap();

        // Man page errors
        static ref MANPAGE_RE: Regex = Regex::new(&format!(r"^{} ([^\[]*)", *PATH_MATCH)).unwrap();
        static ref GROFF_RE: Regex = Regex::new(&format!(r"^{} ([0-9]+): (.+)$", *PATH_MATCH)).unwrap();

        // Version substvar
        static ref VERSION_SUBSTVAR_RE: Regex = Regex::new(&format!(r"([^ ]+) \(line {}\) (.*)", *LINENO_MATCH)).unwrap();
    }

    match tag {
        "autotools-pkg-config-macro-not-cross-compilation-safe" => {
            if let Some(caps) = PURE_FLN_WILDCARD_RE.captures(info) {
                return format!("* [{}:{}]", &caps["path"], &caps["lineno"]);
            }
        }
        "debian-rules-parses-dpkg-parsechangelog"
        | "global-files-wildcard-not-first-paragraph-in-dep5-copyright" => {
            if let Some(caps) = PURE_FLN_RE.captures(info) {
                return format!("[{}:{}]", &caps["path"], &caps["lineno"]);
            }
        }
        "debian-rules-should-not-use-custom-compression-settings" => {
            if let Some(caps) = RULES_LINENO_RE.captures(info) {
                return format!("{} [debian/rules:{}]", &caps[1], &caps["lineno"]);
            }
        }
        "debian-source-options-has-custom-compression-settings" => {
            if let Some(caps) = SOURCE_OPTIONS_RE.captures(info) {
                return format!("{} [debian/source/options:{}]", &caps[1], &caps["lineno"]);
            }
        }
        "missing-license-paragraph-in-dep5-copyright"
        | "missing-license-text-in-dep5-copyright" => {
            // Apply multiple copyright transformations in order
            let mut result = info.to_string();
            if let Some(caps) = COPYRIGHT_LINE_RE.captures(&result) {
                result = format!("{} [debian/copyright:{}]", &caps[1], &caps["lineno"]);
            } else if let Some(caps) = COPYRIGHT_WILDCARD_RE.captures(&result) {
                result = format!("{} [debian/copyright:*]", &caps[1]);
            } else if COPYRIGHT_STAR_RE.is_match(&result) {
                result = "* [debian/copyright:*]".to_string();
            } else if let Some(caps) = COPYRIGHT_SIMPLE_RE.captures(&result) {
                result = format!("{} [debian/copyright:*]", &caps[1]);
            }
            return result;
        }
        "unused-license-paragraph-in-dep5-copyright" => {
            let re = Regex::new(&format!(r"([^ ]+) (.*) \(line {}\)", *LINENO_MATCH)).unwrap();
            if let Some(caps) = re.captures(info) {
                return format!("{} [{}:{}]", &caps[2], &caps[1], &caps["lineno"]);
            }
        }
        "license-problem-undefined-license" | "incomplete-creative-commons-license" => {
            if let Some(caps) = RULES_LINENO_RE.captures(info) {
                return format!("{} [debian/copyright:{}]", &caps[1], &caps["lineno"]);
            }
        }
        "debhelper-tools-from-autotools-dev-are-deprecated"
        | "debian-rules-sets-dpkg-architecture-variable"
        | "override_dh_auto_test-does-not-check-DEB_BUILD_OPTIONS"
        | "dh-quilt-addon-but-quilt-source-format" => {
            if let Some(caps) = RULES_LINENO_RE.captures(info) {
                return format!("{} [debian/rules:{}]", &caps[1], &caps["lineno"]);
            }
        }
        "uses-deprecated-adttmp" => {
            let re = Regex::new(&format!(r"([^ ]+) \(line {}\)", *LINENO_MATCH)).unwrap();
            if let Some(caps) = re.captures(info) {
                return format!("[{}:{}]", &caps[1], &caps["lineno"]);
            }
        }
        "debian-watch-uses-insecure-uri" => {
            if let Some(caps) = RULES_LINENO_RE.captures(info) {
                return format!("{} [debian/watch:{}]", &caps[1], &caps["lineno"]);
            }
        }
        "uses-dpkg-database-directly"
        | "package-contains-documentation-outside-usr-share-doc"
        | "library-not-linked-against-libc"
        | "executable-in-usr-lib"
        | "executable-not-elf-or-script"
        | "image-file-in-usr-lib"
        | "extra-license-file"
        | "script-not-executable"
        | "shell-script-fails-syntax-check"
        | "source-contains-prebuilt-java-object"
        | "source-contains-prebuilt-windows-binary"
        | "source-contains-prebuilt-doxygen-documentation"
        | "source-contains-prebuilt-wasm-binary"
        | "source-contains-prebuilt-binary"
        | "hardening-no-fortify-functions" => {
            if let Some(caps) = PURE_FN_RE.captures(info) {
                return format!("[{}]", &caps["path"]);
            }
        }
        "non-standard-dir-perm" | "non-standard-file-perm" => {
            if let Some(caps) = NON_STANDARD_PERM_RE.captures(info) {
                return format!("{} != {} [{}]", &caps[2], &caps[3], &caps["path"]);
            }
        }
        "executable-is-not-world-readable" => {
            if let Some(caps) = EXECUTABLE_PERM_RE.captures(info) {
                return format!("{} [{}]", &caps[2], &caps["path"]);
            }
        }
        "setuid-binary" | "elevated-privileges" => {
            if let Some(caps) = SETUID_RE.captures(info) {
                return format!("{} {} [{}]", &caps["mode"], &caps[3], &caps["path"]);
            }
        }
        "manpage-has-errors-from-man" => {
            if let Some(caps) = MANPAGE_RE.captures(info) {
                return format!("{} [{}]", &caps[2], &caps["path"]);
            }
        }
        "groff-message" => {
            if let Some(caps) = GROFF_RE.captures(info) {
                return format!("{}: {} [{}:*]", &caps[2], &caps[3], &caps["path"]);
            }
        }
        "source-contains-prebuilt-javascript-object" => {
            if let Some(caps) = PURE_FN_RE.captures(info) {
                return format!("[{}]", &caps["path"]);
            }
            let line_len_re = Regex::new(r"^(?P<path>[^\[ ].+) line length is .*").unwrap();
            if let Some(caps) = line_len_re.captures(info) {
                return format!("[{}]", &caps["path"]);
            }
        }
        "version-substvar-for-external-package" => {
            if let Some(caps) = VERSION_SUBSTVAR_RE.captures(info) {
                return format!(
                    "{} {} [debian/control:{}]",
                    &caps[1], &caps[3], &caps["lineno"]
                );
            }
        }
        _ => {}
    }

    // No transformation matched, return original info
    info.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_matches_exact() {
        assert!(info_matches("foo", "foo"));
        assert!(!info_matches("foo", "bar"));
    }

    #[test]
    fn test_info_matches_wildcard_simple() {
        assert!(info_matches("*", "anything"));
        assert!(info_matches("*", ""));
        assert!(info_matches("**", "anything"));
    }

    #[test]
    fn test_info_matches_wildcard_prefix() {
        assert!(info_matches("*.js", "file.js"));
        assert!(info_matches("*.js", "path/to/file.js"));
        assert!(!info_matches("*.js", "file.css"));
    }

    #[test]
    fn test_info_matches_wildcard_suffix() {
        assert!(info_matches("debian/*", "debian/control"));
        assert!(info_matches("debian/*", "debian/rules"));
        assert!(!info_matches("debian/*", "other/file"));
    }

    #[test]
    fn test_info_matches_wildcard_middle() {
        assert!(info_matches(
            "[debian/copyright:*]",
            "[debian/copyright:31]"
        ));
        assert!(info_matches(
            "[debian/copyright:*]",
            "[debian/copyright:100]"
        ));
        assert!(!info_matches("[debian/copyright:*]", "[debian/rules:31]"));
        assert!(!info_matches("[debian/copyright:*]", "debian/copyright:31"));
    }

    #[test]
    fn test_info_matches_multiple_wildcards() {
        assert!(info_matches("*.html.*.js", "foo.html.bar.js"));
        assert!(info_matches("*.html.*.js", "foo.html.baz.qux.js"));
        assert!(!info_matches("*.html.*.js", "foo.css.bar.js"));
    }

    #[test]
    fn test_info_matches_wildcard_empty_parts() {
        assert!(info_matches("foo**bar", "foobar"));
        assert!(info_matches("foo**bar", "fooxyzbar"));
    }

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
    fn test_parse_info_with_colon() {
        // Test that info fields containing colons are parsed correctly
        // This was a bug where "X-Python-Version: >= 2.5" would be misparsed
        let text = "ancient-python-version-field X-Python-Version: >= 2.5\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].tag().unwrap().text(),
            "ancient-python-version-field"
        );
        assert_eq!(
            lines[0].info(),
            Some("X-Python-Version: >= 2.5".to_string())
        );
        assert_eq!(lines[0].package_spec(), None);
    }

    #[test]
    fn test_parse_source_prefix_with_info_containing_colon() {
        // Test parsing with explicit "source:" prefix and info containing colon
        let text = "source: ancient-python-version-field X-Python-Version: >= 2.5\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].tag().unwrap().text(),
            "ancient-python-version-field"
        );
        assert_eq!(
            lines[0].info(),
            Some("X-Python-Version: >= 2.5".to_string())
        );
        assert_eq!(
            lines[0].package_spec().unwrap().package_name().unwrap(),
            "source"
        );
    }

    #[test]
    fn test_parse_two_word_non_package_spec() {
        // Test that two words before a colon that don't match package spec pattern
        // are not treated as a package spec
        let text = "some-tag field-name: value\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tag().unwrap().text(), "some-tag");
        assert_eq!(lines[0].info(), Some("field-name: value".to_string()));
        assert_eq!(lines[0].package_spec(), None);
    }
}
