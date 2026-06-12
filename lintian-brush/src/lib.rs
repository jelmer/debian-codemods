//! Lintian Brush - Automated Debian package fixes
//!
//! This crate provides tools for automatically fixing common Lintian issues in Debian packages.

#![deny(missing_docs)]
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use breezyshim::dirty_tracker::DirtyTreeTracker;
use breezyshim::error::Error;
use breezyshim::tree::{TreeChange, WorkingTree};
use breezyshim::workspace::{check_clean_tree, reset_tree_with_dirty_tracker};
use breezyshim::RevisionId;
use debian_analyzer::detect_gbp_dch::{guess_update_changelog, ChangelogBehaviour};
use debian_analyzer::{
    add_changelog_entry, apply_or_revert, get_committer, min_certainty, ApplyError, ChangelogError,
};
use debian_changelog::ChangeLog;
use debian_workspace::Workspace;

/// Built-in fixers for common Lintian issues
pub mod builtin_fixers;
/// Debian helper functions and types
pub mod debhelper;
/// Detector interface
pub mod detector;
/// Diagnostic and action types for the detector/applier split.
pub mod diagnostic;
#[macro_use]
/// Macros for defining fixers
/// Fixer-related types and traits
pub mod fixers;
/// License name mappings and common license directories
pub mod licenses;
/// Lintian overrides parsing and manipulation
pub mod lintian_overrides;
/// Utilities for manipulating debian/rules files
pub mod rules;
/// Upstream metadata handling
pub mod upstream_metadata;
/// VCS URL manipulation utilities (requires the `upstream` feature).
#[cfg(feature = "upstream")]
pub mod vcs;
/// Debian watch file handling
pub mod watch;
// Re-export commonly used types for convenience
pub use debian_analyzer::Certainty;
pub use debversion::Version;
pub use fixers::get_renamed_tags;
// Re-export inventory for macros
pub use inventory;

/// Lintian tag visibility level (matches lintian's own classification)
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum Visibility {
    /// Pedantic tags: issues that are very minor or a matter of style
    #[serde(rename = "pedantic")]
    Pedantic,
    /// Info tags: informational, not necessarily a problem
    #[serde(rename = "info")]
    Info,
    /// Warning tags: likely a problem
    #[serde(rename = "warning")]
    Warning,
    /// Error tags: definitely a problem
    #[serde(rename = "error")]
    Error,
}

impl std::fmt::Display for Visibility {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Visibility::Pedantic => write!(f, "pedantic"),
            Visibility::Info => write!(f, "info"),
            Visibility::Warning => write!(f, "warning"),
            Visibility::Error => write!(f, "error"),
        }
    }
}

impl FromStr for Visibility {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pedantic" => Ok(Visibility::Pedantic),
            "info" => Ok(Visibility::Info),
            "warning" => Ok(Visibility::Warning),
            "error" => Ok(Visibility::Error),
            _ => Err(format!("Invalid visibility: {}", value)),
        }
    }
}

/// Type of Debian package
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum PackageType {
    /// Source package
    #[serde(rename = "source")]
    Source,
    /// Binary package
    #[serde(rename = "binary")]
    Binary,
}

impl FromStr for PackageType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "source" => Ok(PackageType::Source),
            "binary" => Ok(PackageType::Binary),
            _ => Err(format!("Invalid package type: {}", value)),
        }
    }
}

impl std::fmt::Display for PackageType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PackageType::Source => write!(f, "source"),
            PackageType::Binary => write!(f, "binary"),
        }
    }
}

/// A Lintian issue
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct LintianIssue {
    /// Package name
    pub package: Option<String>,
    /// Package type
    pub package_type: Option<PackageType>,
    /// Tag visibility level
    pub visibility: Option<Visibility>,
    /// Lintian tag
    pub tag: Option<String>,
    /// Additional information
    pub info: Option<String>,
}

impl std::fmt::Display for LintianIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // Format: "package source: tag info" or "source: tag info"
        if let Some(ref pkg) = self.package {
            write!(f, "{} ", pkg)?;
        }
        if let Some(ref pt) = self.package_type {
            write!(f, "{}", pt)?;
        } else {
            write!(f, "source")?;
        }
        write!(f, ":")?;
        if let Some(ref tag) = self.tag {
            write!(f, " {}", tag)?;
        }
        if let Some(ref info) = self.info {
            if !info.is_empty() {
                write!(f, " {}", info)?;
            }
        }
        Ok(())
    }
}

impl LintianIssue {
    /// Convert the issue to a JSON value
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "package": self.package,
            "package_type": self.package_type.as_ref().map(|t| t.to_string()),
            "visibility": self.visibility.as_ref().map(|v| v.to_string()),
            "tag": self.tag,
            "info": self.info,
        })
    }

    /// Create a LintianIssue from a JSON value
    pub fn from_json(value: serde_json::Value) -> serde_json::Result<Self> {
        serde_json::from_value(value)
    }

    /// Create a LintianIssue with only a tag
    pub fn just_tag(tag: String) -> Self {
        Self {
            package: None,
            package_type: None,
            visibility: None,
            tag: Some(tag),
            info: None,
        }
    }

    /// Create a source package issue with a tag
    pub fn source(tag: impl Into<String>, visibility: Visibility) -> Self {
        Self {
            package: None,
            package_type: Some(PackageType::Source),
            visibility: Some(visibility),
            tag: Some(tag.into()),
            info: None,
        }
    }

    /// Create a source package issue with a tag and info
    pub fn source_with_info(
        tag: impl Into<String>,
        visibility: Visibility,
        info: Vec<String>,
    ) -> Self {
        let joined = info.join(" ");
        Self {
            package: None,
            package_type: Some(PackageType::Source),
            visibility: Some(visibility),
            tag: Some(tag.into()),
            info: if joined.is_empty() {
                None
            } else {
                Some(joined)
            },
        }
    }

    /// Create a binary package issue with a tag and info
    pub fn binary_with_info(
        package: impl Into<String>,
        tag: impl Into<String>,
        visibility: Visibility,
        info: Vec<String>,
    ) -> Self {
        let joined = info.join(" ");
        Self {
            package: Some(package.into()),
            package_type: Some(PackageType::Binary),
            visibility: Some(visibility),
            tag: Some(tag.into()),
            info: if joined.is_empty() {
                None
            } else {
                Some(joined)
            },
        }
    }

    /// Add info to this issue
    pub fn with_info(mut self, info: Vec<String>) -> Self {
        let joined = info.join(" ");
        self.info = if joined.is_empty() {
            None
        } else {
            Some(joined)
        };
        self
    }

    /// Check if this issue should be fixed (i.e., it's not overridden)
    pub fn should_fix(&self, base_path: &std::path::Path) -> bool {
        use crate::lintian_overrides::{self, OverrideLineMatch};

        for line in lintian_overrides::iter_overrides(base_path) {
            if line.matches_issue(self) {
                return false;
            }
        }

        true
    }
}

/// Error type for parsing Lintian issues
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LintianIssueParseError {
    /// Invalid package type encountered
    InvalidPackageType(String),
}

impl std::fmt::Display for LintianIssueParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LintianIssueParseError::InvalidPackageType(s) => {
                write!(f, "Invalid package type: {}", s)
            }
        }
    }
}

impl std::error::Error for LintianIssueParseError {}

impl TryFrom<&str> for LintianIssue {
    type Error = LintianIssueParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let value = value.trim();
        let package_type;
        let package;
        let after = if let Some((before, after)) = value.split_once(':') {
            if let Some((first, second)) = before.trim().split_once(' ') {
                // Check if the format is "package source:" or "source package:"
                if second == "source" {
                    // Format: "package source:"
                    package_type = Some(PackageType::Source);
                    package = Some(first.to_string());
                } else if second == "binary" {
                    // Format: "package binary:"
                    package_type = Some(PackageType::Binary);
                    package = Some(first.to_string());
                } else if first == "source" {
                    // Format: "source package:"
                    package_type = Some(PackageType::Source);
                    package = Some(second.to_string());
                } else if first == "binary" {
                    // Format: "binary package:"
                    package_type = Some(PackageType::Binary);
                    package = Some(second.to_string());
                } else {
                    return Err(LintianIssueParseError::InvalidPackageType(format!(
                        "{} {}",
                        first, second
                    )));
                }
            } else {
                // No space before colon - check if it's "source:" or "binary:"
                if before == "source" {
                    package_type = Some(PackageType::Source);
                    package = None;
                } else if before == "binary" {
                    package_type = Some(PackageType::Binary);
                    package = None;
                } else {
                    // It's a package name
                    package_type = None;
                    package = Some(before.to_string());
                }
            }
            after
        } else {
            package_type = None;
            package = None;
            value
        };
        let after = after.trim();
        let (tag, info) = if let Some((tag_str, info_str)) = after.split_once(' ') {
            let info = if info_str.is_empty() {
                None
            } else {
                Some(info_str.to_string())
            };
            (Some(tag_str.to_string()), info)
        } else {
            (Some(after.to_string()), None)
        };
        Ok(Self {
            package,
            package_type,
            visibility: None,
            tag,
            info,
        })
    }
}

/// Result of running a fixer
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct FixerResult {
    /// Description of the changes made
    pub description: String,
    /// Certainty level of the fix
    pub certainty: Option<Certainty>,
    /// Name of the patch if one was created
    pub patch_name: Option<String>,
    /// Revision ID of the commit
    pub revision_id: Option<RevisionId>,
    /// List of Lintian issues that were fixed
    pub fixed_lintian_issues: Vec<LintianIssue>,
    /// List of Lintian issues that were overridden
    pub overridden_lintian_issues: Vec<LintianIssue>,
}

impl FixerResult {
    /// Create a new FixerResult
    pub fn new(
        description: String,
        certainty: Option<Certainty>,
        patch_name: Option<String>,
        revision_id: Option<RevisionId>,
        fixed_lintian_issues: Vec<LintianIssue>,
        overridden_lintian_issues: Option<Vec<LintianIssue>>,
    ) -> Self {
        Self {
            description,
            certainty,
            patch_name,
            revision_id,
            fixed_lintian_issues,
            overridden_lintian_issues: overridden_lintian_issues.unwrap_or_default(),
        }
    }
    /// Get the list of fixed Lintian tags
    pub fn fixed_lintian_tags(&self) -> Vec<&str> {
        self.fixed_lintian_issues
            .iter()
            .filter_map(|issue| issue.tag.as_deref())
            .collect()
    }

    /// Create a builder for constructing a FixerResult
    pub fn builder(description: impl Into<String>) -> FixerResultBuilder {
        FixerResultBuilder::new(description)
    }
}

/// Builder for constructing FixerResult instances
#[derive(Debug, Default)]
pub struct FixerResultBuilder {
    /// Description of the changes made
    description: String,
    /// Certainty level of the fix
    certainty: Option<Certainty>,
    /// Name of the patch if one was created
    patch_name: Option<String>,
    /// Revision ID of the commit
    revision_id: Option<RevisionId>,
    /// List of Lintian issues that were fixed
    fixed_lintian_issues: Vec<LintianIssue>,
    /// List of Lintian issues that were overridden
    overridden_lintian_issues: Vec<LintianIssue>,
}

impl FixerResultBuilder {
    /// Create a new builder with the required description
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            ..Default::default()
        }
    }

    /// Set the certainty level
    pub fn certainty(mut self, certainty: Certainty) -> Self {
        self.certainty = Some(certainty);
        self
    }

    /// Set the patch name
    pub fn patch_name(mut self, patch_name: impl Into<String>) -> Self {
        self.patch_name = Some(patch_name.into());
        self
    }

    /// Set the revision ID
    pub fn revision_id(mut self, revision_id: RevisionId) -> Self {
        self.revision_id = Some(revision_id);
        self
    }

    /// Add a fixed lintian issue
    pub fn fixed_issue(mut self, issue: LintianIssue) -> Self {
        self.fixed_lintian_issues.push(issue);
        self
    }

    /// Add multiple fixed lintian issues
    pub fn fixed_issues(mut self, issues: impl IntoIterator<Item = LintianIssue>) -> Self {
        self.fixed_lintian_issues.extend(issues);
        self
    }

    /// Add an overridden lintian issue
    pub fn overridden_issue(mut self, issue: LintianIssue) -> Self {
        self.overridden_lintian_issues.push(issue);
        self
    }

    /// Add multiple overridden lintian issues
    pub fn overridden_issues(mut self, issues: impl IntoIterator<Item = LintianIssue>) -> Self {
        self.overridden_lintian_issues.extend(issues);
        self
    }

    /// Build the FixerResult
    pub fn build(self) -> FixerResult {
        FixerResult {
            description: self.description,
            certainty: self.certainty,
            patch_name: self.patch_name,
            revision_id: self.revision_id,
            fixed_lintian_issues: self.fixed_lintian_issues,
            overridden_lintian_issues: self.overridden_lintian_issues,
        }
    }
}

/// Error type for parsing fixer output
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OutputParseError {
    /// Unsupported certainty level encountered
    UnsupportedCertainty(String),
    /// Error parsing a Lintian issue
    LintianIssueParseError(LintianIssueParseError),
}

impl std::fmt::Display for OutputParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            OutputParseError::UnsupportedCertainty(s) => {
                write!(f, "Unsupported certainty: {}", s)
            }
            OutputParseError::LintianIssueParseError(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for OutputParseError {}

impl From<LintianIssueParseError> for OutputParseError {
    fn from(value: LintianIssueParseError) -> Self {
        Self::LintianIssueParseError(value)
    }
}

/// Whether `base_path` is a debcargo-managed package (Rust crate
/// packaging where `debian/control` is generated from `debian/debcargo.toml`).
pub fn is_debcargo_package(base_path: &std::path::Path) -> bool {
    base_path.join("debian/debcargo.toml").exists()
}

/// Parse the output of a script fixer
pub fn parse_script_fixer_output(text: &str) -> Result<FixerResult, OutputParseError> {
    let mut description: Vec<String> = Vec::new();
    let mut overridden_issues: Vec<LintianIssue> = Vec::new();
    let mut fixed_lintian_issues: Vec<LintianIssue> = Vec::new();
    let mut certainty: Option<String> = None;
    let mut patch_name: Option<String> = None;

    let lines: Vec<&str> = text.split_terminator('\n').collect();
    let mut i = 0;

    while i < lines.len() {
        if let Some((key, value)) = lines[i].split_once(':') {
            match key.trim() {
                "Fixed-Lintian-Issues" => {
                    i += 1;
                    while i < lines.len() && lines[i].starts_with(' ') {
                        fixed_lintian_issues.push(LintianIssue::try_from(&lines[i][1..])?);
                        i += 1;
                    }
                    continue;
                }
                "Overridden-Lintian-Issues" => {
                    i += 1;
                    while i < lines.len() && lines[i].starts_with(' ') {
                        overridden_issues.push(LintianIssue::try_from(&lines[i][1..])?);
                        i += 1;
                    }
                    continue;
                }
                "Certainty" => {
                    certainty = Some(value.trim().to_owned());
                }
                "Patch-Name" => {
                    patch_name = Some(value.trim().to_owned());
                }
                _ => {
                    description.push(lines[i].to_owned());
                }
            }
        } else {
            description.push(lines[i].to_owned());
        }

        i += 1;
    }

    let certainty = certainty
        .map(|c| c.parse())
        .transpose()
        .map_err(OutputParseError::UnsupportedCertainty)?;

    let overridden_issues = if overridden_issues.is_empty() {
        None
    } else {
        Some(overridden_issues)
    };

    Ok(FixerResult::new(
        description.join("\n"),
        certainty,
        patch_name,
        None,
        fixed_lintian_issues,
        overridden_issues,
    ))
}

/// Determine the environment variables for running a fixer
pub fn determine_env(
    package: &str,
    current_version: &Version,
    preferences: &FixerPreferences,
) -> std::collections::HashMap<String, String> {
    let mut env = std::env::vars().collect::<std::collections::HashMap<_, _>>();
    env.insert("DEB_SOURCE".to_owned(), package.to_owned());
    env.insert("CURRENT_VERSION".to_owned(), current_version.to_string());
    if let Some(compat_release) = preferences.compat_release.as_ref() {
        env.insert("COMPAT_RELEASE".to_owned(), compat_release.to_owned());
    }
    if let Some(upgrade_release) = preferences.upgrade_release.as_ref() {
        env.insert("UPGRADE_RELEASE".to_owned(), upgrade_release.to_owned());
    }
    env.insert(
        "MINIMUM_CERTAINTY".to_owned(),
        preferences
            .minimum_certainty
            .unwrap_or_default()
            .to_string(),
    );
    env.insert(
        "TRUST_PACKAGE".to_owned(),
        preferences.trust_package.unwrap_or(false).to_string(),
    );
    env.insert(
        "REFORMATTING".to_owned(),
        if preferences.allow_reformatting.unwrap_or(false) {
            "allow"
        } else {
            "disallow"
        }
        .to_owned(),
    );
    env.insert(
        "NET_ACCESS".to_owned(),
        if preferences.net_access.unwrap_or(true) {
            "allow"
        } else {
            "disallow"
        }
        .to_owned(),
    );
    env.insert(
        "OPINIONATED".to_owned(),
        if preferences.opinionated.unwrap_or(false) {
            "yes"
        } else {
            "no"
        }
        .to_owned(),
    );
    env.insert(
        "DILIGENCE".to_owned(),
        preferences.diligence.unwrap_or(0).to_string(),
    );

    // Add Python path for subprocess fixers
    let py_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("py");

    if let Ok(existing_pythonpath) = std::env::var("PYTHONPATH") {
        // Prepend our py directory to existing PYTHONPATH
        env.insert(
            "PYTHONPATH".to_owned(),
            format!("{}:{}", py_path.to_string_lossy(), existing_pythonpath),
        );
    } else {
        // Set PYTHONPATH to just our py directory
        env.insert(
            "PYTHONPATH".to_owned(),
            py_path.to_string_lossy().to_string(),
        );
    }

    // Add any extra environment variables from preferences (used in tests)
    if let Some(extra_env) = &preferences.extra_env {
        for (key, value) in extra_env {
            env.insert(key.clone(), value.clone());
        }
    }

    env
}

/// Preferences for running fixers
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FixerPreferences {
    /// Compatibility release (e.g. "stable" or "unstable")
    pub compat_release: Option<String>,
    /// Minimum certainty level required
    pub minimum_certainty: Option<Certainty>,
    /// Whether to run code from the package if necessary
    pub trust_package: Option<bool>,
    /// Whether to allow reformatting of changed files
    pub allow_reformatting: Option<bool>,
    /// Whether to allow network access
    pub net_access: Option<bool>,
    /// Whether to be opinionated
    pub opinionated: Option<bool>,
    /// Level of diligence
    pub diligence: Option<i32>,
    /// Upgrade release target
    pub upgrade_release: Option<String>,
    /// Extra environment variables (used in tests)
    pub extra_env: Option<std::collections::HashMap<String, String>>,
    /// Path to Lintian data directory
    pub lintian_data_path: Option<std::path::PathBuf>,
}

/// Errors that can occur when running a fixer
#[derive(Debug)]
pub enum FixerError {
    /// No changes were made by the fixer
    NoChanges,
    /// No changes were made after applying overrides
    NoChangesAfterOverrides(Vec<LintianIssue>),
    /// The certainty level is not high enough
    NotCertainEnough(Certainty, Option<Certainty>, Vec<LintianIssue>),
    /// The path is not a Debian package
    NotDebianPackage(std::path::PathBuf),
    /// The description is missing
    DescriptionMissing,
    /// Invalid changelog file
    InvalidChangelog(std::path::PathBuf, String),
    /// Fixer script was not found
    ScriptNotFound(std::path::PathBuf),
    /// Error parsing fixer output
    OutputParseError(OutputParseError),
    /// Failed to manipulate patch
    FailedPatchManipulation(String),
    /// Error creating changelog
    ChangelogCreate(String),
    /// Fixer script failed
    ScriptFailed {
        /// Path to the script
        path: std::path::PathBuf,
        /// Exit code
        exit_code: i32,
        /// Standard error output
        stderr: String,
    },
    /// Formatting could not be preserved
    FormattingUnpreservable(std::path::PathBuf),
    /// File is generated
    GeneratedFile(std::path::PathBuf),
    /// I/O error
    Io(std::io::Error),
    /// Breezy error
    BrzError(Error),
    /// Fixer panicked
    Panic {
        /// Panic message
        message: String,
        /// Backtrace if available
        backtrace: Option<std::backtrace::Backtrace>,
    },
    /// Missing optional dependency
    MissingDependency(String),
    /// Other error
    Other(String),
}

impl From<debian_analyzer::editor::EditorError> for FixerError {
    fn from(e: debian_analyzer::editor::EditorError) -> Self {
        match e {
            debian_analyzer::editor::EditorError::IoError(e) => e.into(),
            debian_analyzer::editor::EditorError::BrzError(e) => e.into(),
            debian_analyzer::editor::EditorError::GeneratedFile(p, _) => {
                FixerError::GeneratedFile(p)
            }
            debian_analyzer::editor::EditorError::FormattingUnpreservable(p, _e) => {
                FixerError::FormattingUnpreservable(p)
            }
            debian_analyzer::editor::EditorError::TemplateError(p, _e) => {
                FixerError::GeneratedFile(p)
            }
        }
    }
}

impl From<std::io::Error> for FixerError {
    fn from(e: std::io::Error) -> Self {
        FixerError::Io(e)
    }
}

impl From<debian_changelog::Error> for FixerError {
    fn from(e: debian_changelog::Error) -> Self {
        match e {
            debian_changelog::Error::Io(e) => FixerError::Io(e),
            debian_changelog::Error::Parse(e) => FixerError::ChangelogCreate(e.to_string()),
        }
    }
}

impl From<debian_changelog::ParseError> for FixerError {
    fn from(e: debian_changelog::ParseError) -> Self {
        FixerError::ChangelogCreate(e.to_string())
    }
}

impl From<ChangelogError> for FixerError {
    fn from(e: ChangelogError) -> Self {
        match e {
            ChangelogError::NotDebianPackage(path) => FixerError::NotDebianPackage(path),
            ChangelogError::Python(e) => FixerError::Other(e.to_string()),
        }
    }
}

impl From<Error> for FixerError {
    fn from(e: Error) -> Self {
        FixerError::BrzError(e)
    }
}

impl From<debian_workspace::Error> for FixerError {
    fn from(e: debian_workspace::Error) -> Self {
        match e {
            debian_workspace::Error::NotFound => FixerError::NoChanges,
            debian_workspace::Error::Io(e) => FixerError::Io(e),
            debian_workspace::Error::Parse(msg) | debian_workspace::Error::Other(msg) => {
                FixerError::Other(msg)
            }
            debian_workspace::Error::MissingDependency(dep) => FixerError::MissingDependency(dep),
        }
    }
}

impl From<OutputParseError> for FixerError {
    fn from(e: OutputParseError) -> Self {
        FixerError::OutputParseError(e)
    }
}

impl std::fmt::Display for FixerError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FixerError::NoChanges => write!(f, "No changes"),
            FixerError::NoChangesAfterOverrides(_) => write!(f, "No changes after overrides"),
            FixerError::OutputParseError(e) => write!(f, "Output parse error: {}", e),
            FixerError::ScriptNotFound(p) => write!(f, "Command not found: {}", p.display()),
            FixerError::ChangelogCreate(m) => write!(f, "Changelog create error: {}", m),
            FixerError::FormattingUnpreservable(p) => {
                write!(f, "Formatting unpreservable for {}", p.display())
            }
            FixerError::ScriptFailed {
                path,
                exit_code,
                stderr,
            } => write!(
                f,
                "Script failed: {} (exit code {}) (stderr: {})",
                path.display(),
                exit_code,
                stderr
            ),
            FixerError::Other(s) => write!(f, "{}", s),
            FixerError::NotDebianPackage(p) => write!(f, "Not a Debian package: {}", p.display()),
            FixerError::DescriptionMissing => {
                write!(f, "Description missing")
            }
            FixerError::NotCertainEnough(actual, minimum, _) => write!(
                f,
                "Not certain enough to fix (actual: {}, minimum : {:?})",
                actual, minimum
            ),
            FixerError::Io(e) => write!(f, "IO error: {}", e),
            FixerError::FailedPatchManipulation(s) => {
                write!(f, "Failed to manipulate patch: {}", s)
            }
            FixerError::BrzError(e) => write!(f, "Breezy error: {}", e),
            FixerError::InvalidChangelog(p, s) => {
                write!(f, "Invalid changelog {}: {}", p.display(), s)
            }
            FixerError::GeneratedFile(p) => write!(f, "Generated file: {}", p.display()),
            FixerError::Panic { message, backtrace } => {
                write!(f, "Panic: {}", message)?;
                if let Some(bt) = backtrace {
                    write!(f, "\nBacktrace:\n{}", bt)?;
                }
                Ok(())
            }
            FixerError::MissingDependency(dep) => write!(f, "Missing optional dependency: {}", dep),
        }
    }
}

impl std::error::Error for FixerError {}

/// Return a list of all lintian fixers.
///
/// Each item is a registered [`Detector`](crate::detector::Detector),
/// sorted by its `after` / `before` declarations.
pub fn all_lintian_fixers() -> impl Iterator<Item = Box<dyn crate::detector::Detector>> {
    builtin_fixers::get_builtin_fixers().into_iter()
}

/// Default value for addon-only fixes
pub const DEFAULT_VALUE_LINTIAN_BRUSH_ADDON_ONLY: i32 = 10;
/// Default value for lintian-brush fixes
pub const DEFAULT_VALUE_LINTIAN_BRUSH: i32 = 50;
/// Tag-specific values
pub const LINTIAN_BRUSH_TAG_VALUES: [(&str, i32); 1] = [("trailing-whitespace", 0)];
/// Default addon fixers
pub const DEFAULT_ADDON_FIXERS: &[&str] = &[
    "debian-changelog-line-too-long",
    "trailing-whitespace",
    "out-of-date-standards-version",
    "package-uses-old-debhelper-compat-version",
    "public-upstream-key-not-minimal",
];
/// Default value for lintian-brush tags
pub const LINTIAN_BRUSH_TAG_DEFAULT_VALUE: i32 = 5;

/// Calculate the value of a set of tags
pub fn calculate_value(tags: &[&str]) -> i32 {
    if tags.is_empty() {
        return 0;
    }

    let default_addon_fixers: HashSet<&str> = DEFAULT_ADDON_FIXERS.iter().cloned().collect();
    let tag_set: HashSet<&str> = tags.iter().cloned().collect();

    if tag_set.is_subset(&default_addon_fixers) {
        return DEFAULT_VALUE_LINTIAN_BRUSH_ADDON_ONLY;
    }

    let mut value = DEFAULT_VALUE_LINTIAN_BRUSH;

    for tag in tags {
        if let Some(tag_value) = LINTIAN_BRUSH_TAG_VALUES.iter().find(|(t, _)| t == tag) {
            value += tag_value.1;
        } else {
            value += LINTIAN_BRUSH_TAG_DEFAULT_VALUE;
        }
    }

    value
}

/// Find the path to a data file
pub fn data_file_path(
    name: &str,
    check: impl Fn(&std::path::Path) -> bool,
) -> Option<std::path::PathBuf> {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path = path.join("..").join(name);
    if check(&path) {
        return Some(path);
    }

    let base_paths = &["/usr/share/lintian-brush", "/usr/local/share/lintian-brush"];

    for base_path in base_paths {
        let path = std::path::Path::new(base_path).join(name);
        if check(&path) {
            return Some(path);
        }
    }

    None
}

/// Render the `Fixes:` and `See-also:` trailer block for a set of fixed Lintian issues.
///
/// One `Fixes:` line is emitted per issue (carrying package, type and info via
/// [`LintianIssue`]'s `Display` impl). One `See-also:` line is emitted per unique
/// tag, in the order the tag is first encountered.
pub fn render_lintian_trailers(issues: &[LintianIssue]) -> String {
    let mut out = String::new();
    for issue in issues {
        out.push_str(&format!("Fixes: lintian: {}\n", issue));
    }
    let mut seen = std::collections::HashSet::new();
    for issue in issues {
        if let Some(tag) = issue.tag.as_deref() {
            if seen.insert(tag) {
                out.push_str(&format!(
                    "See-also: https://lintian.debian.org/tags/{}.html\n",
                    tag
                ));
            }
        }
    }
    out
}

/// Run a lintian detector on a tree.
///
/// # Arguments
///
///  * `local_tree`: WorkingTree object
///  * `basis_tree`: Tree
///  * `detector`: Detector object to apply
///  * `committer`: Optional committer (name and email)
///  * `update_changelog`: Whether to add a new entry to the changelog
///  * `compat_release`: Minimum release that the package should be usable on
///  * `  (e.g. 'stable' or 'unstable')
///  * `minimum_certainty`: How certain the fixer should be
///  * `  about its changes.
///  * `trust_package`: Whether to run code from the package if necessary
///  * `allow_reformatting`: Whether to allow reformatting of changed files
///  * `dirty_tracker`: Optional object that can be used to tell if the tree
///  * `  has been changed.
///  * `subpath`: Path in tree to operate on
///  * `net_access`: Whether to allow accessing external services
///  * `opinionated`: Whether to be opinionated
///  * `diligence`: Level of diligence
///
/// # Returns
///   tuple with set of FixerResult, summary of the changes
pub fn run_lintian_fixer(
    local_tree: &breezyshim::workingtree::GenericWorkingTree,
    detector: &dyn crate::detector::Detector,
    committer: Option<&str>,
    mut update_changelog: impl FnMut() -> bool,
    preferences: &FixerPreferences,
    dirty_tracker: &mut Option<DirtyTreeTracker>,
    subpath: &std::path::Path,
    timestamp: Option<chrono::naive::NaiveDateTime>,
    basis_tree: Option<&dyn breezyshim::tree::PyTree>,
    changes_by: Option<&str>,
) -> Result<(FixerResult, String), FixerError> {
    let changes_by = changes_by.unwrap_or("lintian-brush");

    let changelog_path = subpath.join("debian/changelog");

    let basedir = local_tree.abspath(subpath).unwrap();
    let cl = match debian_workspace::fs_workspace::FsWorkspace::new(basedir.as_path(), None, None)
        .parsed_changelog()
    {
        Ok(cl) => cl,
        Err(debian_workspace::Error::NotFound) => {
            return Err(FixerError::NotDebianPackage(basedir));
        }
        Err(e) => return Err(FixerError::Other(e.to_string())),
    };
    let first_entry = if let Some(entry) = cl.iter().next() {
        entry
    } else {
        return Err(FixerError::InvalidChangelog(
            basedir,
            "No entries in changelog".to_string(),
        ));
    };
    let package = first_entry.package().unwrap();
    let current_version: Version =
        if first_entry.distributions().as_deref().unwrap() == vec!["UNRELEASED"] {
            first_entry.version().unwrap()
        } else {
            let mut version = first_entry.version().unwrap();
            version.increment_debian();
            version
        };

    let mut _bt: Option<breezyshim::tree::RevisionTree> = None;
    let basis_tree = if let Some(_basis_tree) = basis_tree {
        // For now, we'll use the local tree's basis_tree since converting trait objects is complex
        local_tree.basis_tree().unwrap()
    } else {
        local_tree.basis_tree().unwrap()
    };

    let ws = debian_workspace::fs_workspace::FsWorkspace::new(
        basedir.as_path(),
        Some(package.to_string()),
        Some(current_version.clone()),
    );

    // Detect first: run the detector and filter its diagnostics into a
    // plan. Only if something actionable survives do we enter
    // `apply_or_revert` and mutate the tree.
    tracing::debug!("Running detector {}", detector.name());
    let plan = crate::detector::detect_and_plan(detector, &ws, preferences)?;

    let make_changes = |basedir: &std::path::Path| -> Result<_, FixerError> {
        crate::builtin_fixers::apply_plan(basedir, plan, &|fixed, actions| {
            detector.describe(fixed, actions)
        })
    };

    let (mut result, changes, mut specific_files) = match apply_or_revert(
        local_tree,
        subpath,
        &basis_tree,
        dirty_tracker.as_mut(),
        make_changes,
    ) {
        Ok(r) => {
            if r.0.description.is_empty() {
                return Err(FixerError::DescriptionMissing);
            }

            r
        }
        Err(ApplyError::NoChanges(r)) => {
            if r.overridden_lintian_issues.is_empty() {
                return Err(FixerError::NoChanges);
            } else {
                return Err(FixerError::NoChangesAfterOverrides(
                    r.overridden_lintian_issues,
                ));
            }
        }
        Err(ApplyError::BrzError(e)) => {
            return Err(e.into());
        }
        Err(ApplyError::CallbackError(e)) => {
            return Err(e);
        }
    };

    let lines = result.description.split('\n').collect::<Vec<_>>();
    let mut summary = lines[0].to_string();
    let details = lines
        .iter()
        .skip(1)
        .take_while(|l| !l.is_empty())
        .collect::<Vec<_>>();

    // If there are upstream changes in a non-native package, perhaps
    // export them to debian/patches
    if has_non_debian_changes(changes.as_slice(), subpath)
        && current_version.debian_revision.is_some()
    {
        let (patch_name, updated_specific_files) = match upstream_changes_to_patch(
            local_tree,
            &basis_tree,
            dirty_tracker.as_mut(),
            subpath,
            &result
                .patch_name
                .as_deref()
                .map_or_else(|| detector.name().to_string(), |n| n.to_string()),
            result.description.as_str(),
            timestamp.map(|t| t.date()),
        ) {
            Ok(r) => r,
            Err(e) => {
                reset_tree_with_dirty_tracker(
                    local_tree,
                    Some(&basis_tree),
                    Some(subpath),
                    dirty_tracker.as_mut(),
                )
                .map_err(|e| FixerError::Other(e.to_string()))?;

                return Err(FixerError::FailedPatchManipulation(e.to_string()));
            }
        };

        specific_files = Some(updated_specific_files);

        summary = format!("Add patch {}: {}", patch_name, summary);
    }

    let update_changelog = if debian_analyzer::changelog::only_changes_last_changelog_block(
        local_tree,
        &basis_tree,
        changelog_path.as_path(),
        changes.iter(),
    )? {
        // If the script only changed the last entry in the changelog,
        // don't update the changelog
        false
    } else {
        update_changelog()
    };

    if update_changelog {
        let summary_with_prefix = format!("* {}", summary);
        let details_with_prefix: Vec<String> = details.iter().map(|d| format!("* {}", d)).collect();

        let mut entry = vec![summary_with_prefix.as_str()];
        entry.extend(details_with_prefix.iter().map(|s| s.as_str()));

        add_changelog_entry(local_tree, changelog_path.as_path(), entry.as_slice())?;
        if let Some(specific_files) = specific_files.as_mut() {
            specific_files.push(changelog_path);
        }
    }

    let mut description = format!("{}\n", result.description);
    description.push('\n');
    description.push_str(format!("Changes-By: {}\n", changes_by).as_str());
    description.push_str(&render_lintian_trailers(&result.fixed_lintian_issues));

    let committer = committer.map_or_else(|| get_committer(local_tree), |c| c.to_string());

    let specific_files_ref = specific_files
        .as_ref()
        .map(|fs| fs.iter().map(|p| p.as_path()).collect::<Vec<_>>());

    let mut builder = local_tree
        .build_commit()
        .message(description.as_str())
        .allow_pointless(false)
        .committer(committer.as_str());

    if let Some(specific_files_ref) = specific_files_ref.as_ref() {
        builder = builder.specific_files(specific_files_ref);
    }

    let revid = builder.commit().map_err(|e| match e {
        Error::PointlessCommit => FixerError::NoChanges,
        Error::NoWhoami => FixerError::Other("No committer specified".to_string()),
        e => FixerError::Other(e.to_string()),
    })?;
    result.revision_id = Some(revid);

    // TODO(jelmer): Support running sbuild & verify lintian warning is gone?
    Ok((result, summary))
}

/// Overall errors that can occur when running fixers
#[derive(Debug)]
pub enum OverallError {
    /// Not a Debian package
    NotDebianPackage(std::path::PathBuf),
    /// Workspace is dirty
    WorkspaceDirty(std::path::PathBuf),
    /// Error creating changelog
    ChangelogCreate(String),
    /// Invalid changelog file
    InvalidChangelog(std::path::PathBuf, String),
    /// Breezy error
    BrzError(Error),
    /// I/O error
    IoError(std::io::Error),
    /// Other error
    Other(String),
}

impl std::fmt::Display for OverallError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            OverallError::NotDebianPackage(path) => {
                write!(f, "Not a Debian package: {}", path.display())
            }
            OverallError::WorkspaceDirty(path) => {
                write!(f, "Workspace is dirty: {}", path.display())
            }
            OverallError::ChangelogCreate(m) => {
                write!(f, "Failed to create changelog entry: {}", m)
            }
            OverallError::Other(e) => write!(f, "{}", e),
            OverallError::BrzError(e) => write!(f, "{}", e),
            OverallError::IoError(e) => write!(f, "{}", e),
            OverallError::InvalidChangelog(path, e) => {
                write!(f, "Invalid changelog at {}: {}", path.display(), e)
            }
        }
    }
}

impl std::error::Error for OverallError {}

/// Run a set of lintian fixers on a tree.
///
/// # Arguments
///
///  * `tree`: The tree to run the detectors on
///  * `detectors`: A set of Detector objects
///  * `update_changelog`: Whether to add an entry to the changelog
///  * `verbose`: Whether to be verbose
///  * `committer`: Optional committer (name and email)
///  * `compat_release`: Minimum release that the package should be usable on
///    (e.g. 'sid' or 'stretch')
///  * `minimum_certainty`: How certain the fixer should be about its changes.
///  * `trust_package`: Whether to run code from the package if necessary
///  * `allow_reformatting`: Whether to allow reformatting of changed files
///  * `use_inotify`: Use inotify to watch changes (significantly improves
///    performance). Defaults to None (automatic)
///  * `subpath`: Subpath in the tree in which the package lives
///  * `net_access`: Whether to allow network access
///  * `opinionated`: Whether to be opinionated
///  * `diligence`: Level of diligence
///  * `changes_by`: Name of the person making the changes
///
/// # Returns:
///   Tuple with two lists:
///     1. list of tuples with (lintian-tag, certainty, description) of fixers
///        that ran
///     2. dictionary mapping fixer names for fixers that failed to run to the
///        error that occurred
pub fn run_lintian_fixers(
    local_tree: &breezyshim::workingtree::GenericWorkingTree,
    detectors: &[Box<dyn crate::detector::Detector>],
    mut update_changelog: Option<impl FnMut() -> bool>,
    verbose: bool,
    committer: Option<&str>,
    preferences: &FixerPreferences,
    use_dirty_tracker: Option<bool>,
    subpath: Option<&std::path::Path>,
    changes_by: Option<&str>,
    multi_progress: Option<&MultiProgress>,
) -> Result<ManyResult, OverallError> {
    let subpath = subpath.unwrap_or_else(|| std::path::Path::new(""));
    let mut basis_tree = local_tree.basis_tree().unwrap();
    check_clean_tree(local_tree, &basis_tree, subpath).map_err(|e| match e {
        Error::WorkspaceDirty(p) => OverallError::WorkspaceDirty(p),
        e => OverallError::Other(e.to_string()),
    })?;

    let mut changelog_behaviour = None;

    // If we don't know whether to update the changelog, then find out *once*
    let mut update_changelog = || {
        if let Some(update_changelog) = update_changelog.as_mut() {
            return update_changelog();
        }
        let debian_path = subpath.join("debian");
        let cb = determine_update_changelog(local_tree, debian_path.as_path());
        changelog_behaviour = Some(cb);
        changelog_behaviour.as_ref().unwrap().update_changelog
    };

    let mut ret = ManyResult::new();
    let pb = if let Some(mp) = multi_progress {
        mp.add(ProgressBar::new(detectors.len() as u64))
    } else {
        ProgressBar::new(detectors.len() as u64)
    };
    pb.set_style(
        ProgressStyle::with_template("[{pos}/{len}] {wide_bar} {msg}")
            .expect("static template is valid"),
    );
    #[cfg(test)]
    pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    let mut dirty_tracker = if use_dirty_tracker.unwrap_or(true) {
        Some(DirtyTreeTracker::new_in_subpath(
            Clone::clone(local_tree),
            subpath,
        ))
    } else {
        None
    };
    for detector in detectors {
        let fixer_name = detector.name();
        pb.set_message(fixer_name);
        // Get now from chrono
        let start = std::time::SystemTime::now();
        if let Some(dirty_tracker) = dirty_tracker.as_mut() {
            dirty_tracker.mark_clean();
        }
        pb.inc(1);

        // Create a span for this fixer so log messages are attributed to it
        let _span = tracing::info_span!("fixer", name = fixer_name).entered();

        match run_lintian_fixer(
            local_tree,
            detector.as_ref(),
            committer,
            &mut update_changelog,
            preferences,
            &mut dirty_tracker,
            subpath,
            None,
            Some(&basis_tree),
            changes_by,
        ) {
            Err(e) => match e {
                FixerError::NotDebianPackage(path) => {
                    return Err(OverallError::NotDebianPackage(path));
                }
                FixerError::ChangelogCreate(ref _m) => {
                    ret.failed_fixers
                        .insert(fixer_name.to_string(), e.to_string());
                    if verbose {
                        tracing::info!("Fixer {} failed to create changelog entry.", fixer_name);
                    }
                    continue;
                }
                FixerError::OutputParseError(ref _e) => {
                    ret.failed_fixers
                        .insert(fixer_name.to_string(), e.to_string());
                    if verbose {
                        tracing::info!("Fixer {} failed to parse output.", fixer_name);
                    }
                    continue;
                }
                FixerError::DescriptionMissing => {
                    ret.failed_fixers
                        .insert(fixer_name.to_string(), e.to_string());
                    if verbose {
                        tracing::info!(
                            "Fixer {} failed because description is missing.",
                            fixer_name
                        );
                    }
                    continue;
                }
                FixerError::FormattingUnpreservable(path) => {
                    ret.formatting_unpreservable
                        .insert(fixer_name.to_string(), path.clone());
                    if verbose {
                        tracing::info!(
                            "Fixer {} was unable to preserve formatting of {}.",
                            fixer_name,
                            path.display()
                        );
                    }
                    continue;
                }
                FixerError::GeneratedFile(p) => {
                    ret.failed_fixers.insert(
                        fixer_name.to_string(),
                        format!("Generated file: {}", p.display()),
                    );
                    if verbose {
                        tracing::info!(
                            "Fixer {} encountered generated file {}",
                            fixer_name,
                            p.display()
                        );
                    }
                }
                FixerError::ScriptNotFound(ref p) => {
                    ret.failed_fixers
                        .insert(fixer_name.to_string(), e.to_string());
                    if verbose {
                        tracing::info!("Fixer {} ({}) not found.", fixer_name, p.display());
                    }
                    continue;
                }
                FixerError::ScriptFailed { .. } => {
                    ret.failed_fixers
                        .insert(fixer_name.to_string(), e.to_string());
                    if verbose {
                        tracing::info!("Fixer {} failed to run.", fixer_name);
                        eprintln!("{}", e);
                    }
                    continue;
                }
                FixerError::BrzError(e) => {
                    return Err(OverallError::BrzError(e));
                }
                FixerError::Io(e) => {
                    tracing::error!("Fixer {} hit I/O error: {}", fixer_name, e);
                    return Err(OverallError::IoError(e));
                }
                FixerError::NotCertainEnough(actual_certainty, minimum_certainty, _overrides) => {
                    let duration = std::time::SystemTime::now().duration_since(start).unwrap();
                    ret.fixer_durations.insert(fixer_name.to_string(), duration);
                    ret.uncertain_fixers
                        .insert(fixer_name.to_string(), actual_certainty);
                    if verbose {
                        tracing::info!(
                    "Fixer {} made changes but not high enough certainty (was {}, needed {}). (took: {:2}s)",
                    fixer_name,
                    actual_certainty,
                    minimum_certainty.map_or("default".to_string(), |c| c.to_string()),
                    duration.as_secs_f32(),
                );
                    }
                    continue;
                }
                FixerError::FailedPatchManipulation(ref reason) => {
                    if verbose {
                        tracing::info!("Unable to manipulate upstream patches: {}", reason);
                    }
                    ret.failed_fixers
                        .insert(fixer_name.to_string(), e.to_string());
                    continue;
                }
                FixerError::NoChanges => {
                    let duration = std::time::SystemTime::now().duration_since(start).unwrap();
                    ret.fixer_durations.insert(fixer_name.to_string(), duration);
                    if verbose {
                        tracing::info!(
                            "Fixer {} made no changes. (took: {:2}s)",
                            fixer_name,
                            duration.as_secs_f32(),
                        );
                    }
                    continue;
                }
                FixerError::NoChangesAfterOverrides(os) => {
                    let duration = std::time::SystemTime::now().duration_since(start).unwrap();
                    ret.fixer_durations.insert(fixer_name.to_string(), duration);
                    if verbose {
                        tracing::info!(
                            "Fixer {} made no changes. (took: {:2}s)",
                            fixer_name,
                            duration.as_secs_f32(),
                        );
                    }
                    ret.overridden_lintian_issues.extend(os);
                    continue;
                }
                FixerError::Panic {
                    ref message,
                    ref backtrace,
                } => {
                    if verbose {
                        tracing::error!("Fixer {} panicked: {}", fixer_name, message);
                        if let Some(bt) = backtrace {
                            tracing::error!("Backtrace:\n{}", bt);
                        }
                    }
                    ret.failed_fixers
                        .insert(fixer_name.to_string(), e.to_string());
                    continue;
                }
                FixerError::MissingDependency(ref dep) => {
                    if verbose {
                        tracing::info!(
                            "Fixer {} skipped: missing optional dependency '{}'",
                            fixer_name,
                            dep
                        );
                    }
                    ret.failed_fixers
                        .insert(fixer_name.to_string(), e.to_string());
                    continue;
                }
                FixerError::Other(ref em) => {
                    if verbose {
                        tracing::info!("Fixer {} failed: {}", fixer_name, em);
                    }
                    ret.failed_fixers
                        .insert(fixer_name.to_string(), e.to_string());
                    continue;
                }
                FixerError::InvalidChangelog(path, reason) => {
                    return Err(OverallError::InvalidChangelog(path, reason));
                }
            },
            Ok((result, summary)) => {
                let duration = std::time::SystemTime::now().duration_since(start).unwrap();
                ret.fixer_durations.insert(fixer_name.to_string(), duration);
                if verbose {
                    tracing::info!(
                        "Fixer {} made changes. (took {:2}s)",
                        fixer_name,
                        duration.as_secs_f32(),
                    );
                }
                ret.success.push(FixerSuccess {
                    result,
                    summary,
                    fixer_name: fixer_name.to_string(),
                });
                basis_tree = local_tree.basis_tree().unwrap();
            }
        }
    }
    pb.finish();
    ret.changelog_behaviour = changelog_behaviour;
    Ok(ret)
}

/// Information about a successfully applied fixer
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FixerSuccess {
    /// The result of the fixer
    #[serde(flatten)]
    pub result: FixerResult,
    /// Summary of the changes
    pub summary: String,
    /// Name of the fixer (for looking up duration and other info)
    pub fixer_name: String,
}

/// Result of running multiple fixers
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ManyResult {
    /// Successfully applied fixers
    #[serde(rename = "applied")]
    pub success: Vec<FixerSuccess>,
    /// Failed fixers
    #[serde(rename = "failed")]
    pub failed_fixers: std::collections::HashMap<String, String>,
    /// Fixers that produced a change which was below the requested
    /// minimum certainty and therefore not applied. Maps fixer name to
    /// the certainty its change would have had.
    #[serde(skip)]
    pub uncertain_fixers: std::collections::HashMap<String, Certainty>,
    /// Changelog behaviour
    pub changelog_behaviour: Option<ChangelogBehaviour>,
    /// Overridden Lintian issues
    #[serde(skip)]
    pub overridden_lintian_issues: Vec<LintianIssue>,
    /// Files with unpreservable formatting
    #[serde(skip)]
    pub formatting_unpreservable: std::collections::HashMap<String, std::path::PathBuf>,
    /// Duration of all fixers that were run (by fixer name)
    #[serde(skip)]
    pub fixer_durations: std::collections::HashMap<String, std::time::Duration>,
}

impl ManyResult {
    /// Count of fixed tags
    pub fn tags_count(&self) -> HashMap<&str, u32> {
        self.success
            .iter()
            .fold(HashMap::new(), |mut acc, fixer_success| {
                for tag in fixer_success.result.fixed_lintian_tags() {
                    *acc.entry(tag).or_insert(0) += 1;
                }
                acc
            })
    }

    /// Calculate the total value of all fixed tags
    pub fn value(&self) -> i32 {
        let tags = self
            .success
            .iter()
            .flat_map(|fixer_success| fixer_success.result.fixed_lintian_tags())
            .collect::<Vec<_>>();
        calculate_value(tags.as_slice())
    }

    /// Return the minimum certainty of any successfully made change.
    pub fn minimum_success_certainty(&self) -> Certainty {
        min_certainty(
            self.success
                .iter()
                .filter_map(|fixer_success| fixer_success.result.certainty)
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .unwrap_or(Certainty::Certain)
    }

    /// Create a new empty ManyResult
    pub fn new() -> Self {
        Self {
            success: Vec::new(),
            failed_fixers: std::collections::HashMap::new(),
            uncertain_fixers: std::collections::HashMap::new(),
            changelog_behaviour: None,
            overridden_lintian_issues: Vec::new(),
            formatting_unpreservable: std::collections::HashMap::new(),
            fixer_durations: std::collections::HashMap::new(),
        }
    }
}

fn has_non_debian_changes(changes: &[TreeChange], subpath: &std::path::Path) -> bool {
    let debian_path = subpath.join("debian");
    changes.iter().any(|change| {
        [change.path.0.as_deref(), change.path.1.as_deref()]
            .into_iter()
            .flatten()
            .any(|path| !path.starts_with(&debian_path))
    })
}

#[derive(Debug)]
struct FailedPatchManipulation(String);

impl std::fmt::Display for FailedPatchManipulation {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Failed to manipulate patches: {}", self.0)
    }
}

impl std::error::Error for FailedPatchManipulation {}

fn upstream_changes_to_patch<T: breezyshim::tree::PyTree>(
    local_tree: &breezyshim::workingtree::GenericWorkingTree,
    basis_tree: &T,
    dirty_tracker: Option<&mut DirtyTreeTracker>,
    subpath: &std::path::Path,
    patch_name: &str,
    description: &str,
    timestamp: Option<chrono::naive::NaiveDate>,
) -> Result<(String, Vec<std::path::PathBuf>), FailedPatchManipulation> {
    use debian_analyzer::patches::{
        move_upstream_changes_to_patch, read_quilt_patches, tree_patches_directory,
    };

    // TODO(jelmer): Apply all patches before generating a diff.

    let patches_directory = tree_patches_directory(local_tree, subpath);
    let quilt_patches =
        read_quilt_patches(local_tree, patches_directory.as_path()).collect::<Vec<_>>();
    if !quilt_patches.is_empty() {
        return Err(FailedPatchManipulation(
            "Creating patch on top of existing quilt patches not supported.".to_string(),
        ));
    }

    tracing::debug!("Moving upstream changes to patch {}", patch_name);
    let (specific_files, patch_name) = match move_upstream_changes_to_patch(
        local_tree,
        basis_tree,
        subpath,
        patch_name,
        description,
        dirty_tracker,
        timestamp,
    ) {
        Ok(r) => r,
        Err(e) => {
            return Err(FailedPatchManipulation(e.to_string()));
        }
    };

    Ok((patch_name, specific_files))
}

fn note_changelog_policy(policy: bool, msg: &str) {
    lazy_static::lazy_static! {
        static ref CHANGELOG_POLICY_NOTED: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    }
    if let Ok(mut policy_noted) = CHANGELOG_POLICY_NOTED.lock() {
        if !*policy_noted {
            let extra = if policy {
                "Specify --no-update-changelog to override."
            } else {
                "Specify --update-changelog to override."
            };
            tracing::info!("{} {}", msg, extra);
        }
        *policy_noted = true;
    }
}

/// Determine whether to update the changelog
pub fn determine_update_changelog(
    local_tree: &dyn WorkingTree,
    debian_path: &std::path::Path,
) -> ChangelogBehaviour {
    let changelog_path = debian_path.join("changelog");

    let cl = match local_tree.get_file(changelog_path.as_path()) {
        Ok(f) => ChangeLog::read_relaxed(f).unwrap(),

        Err(Error::NoSuchFile(_)) => {
            // If there's no changelog, then there's nothing to update!
            return ChangelogBehaviour {
                update_changelog: false,
                explanation: "No changelog found".to_string(),
            };
        }
        Err(e) => {
            panic!("Error reading changelog: {}", e);
        }
    };

    let behaviour = guess_update_changelog(local_tree, debian_path, Some(cl));

    let behaviour = if let Some(behaviour) = behaviour {
        note_changelog_policy(behaviour.update_changelog, behaviour.explanation.as_str());
        behaviour
    } else {
        // If we can't make an educated guess, assume yes.
        ChangelogBehaviour {
            update_changelog: true,
            explanation: "Assuming changelog should be updated".to_string(),
        }
    };

    behaviour
}

#[cfg(test)]
mod tests {
    use super::*;
    use breezyshim::controldir::{create_standalone_workingtree, ControlDirFormat};
    use breezyshim::repository::Repository;
    use breezyshim::tree::{MutableTree, Tree, WorkingTree};
    use breezyshim::workingtree::GenericWorkingTree;
    use breezyshim::Branch;
    use std::path::Path;

    pub const COMMITTER: &str = "Testsuite <lintian-brush@example.com>";

    mod render_lintian_trailers_tests {
        use super::*;

        #[test]
        fn empty_issues_renders_empty_string() {
            assert_eq!(render_lintian_trailers(&[]), "");
        }

        #[test]
        fn single_source_tag_without_info() {
            let trailers = render_lintian_trailers(&[LintianIssue {
                package: Some("blah".to_string()),
                package_type: Some(PackageType::Source),
                visibility: None,
                tag: Some("some-tag".to_string()),
                info: None,
            }]);
            assert_eq!(
                trailers,
                "Fixes: lintian: blah source: some-tag\n\
                 See-also: https://lintian.debian.org/tags/some-tag.html\n"
            );
        }

        #[test]
        fn info_string_is_preserved_in_fixes_line() {
            let trailers = render_lintian_trailers(&[LintianIssue {
                package: None,
                package_type: Some(PackageType::Source),
                visibility: None,
                tag: Some("globbing-patterns-out-of-order".to_string()),
                info: Some("debian/*".to_string()),
            }]);
            assert_eq!(
                trailers,
                "Fixes: lintian: source: globbing-patterns-out-of-order debian/*\n\
                 See-also: https://lintian.debian.org/tags/globbing-patterns-out-of-order.html\n"
            );
        }

        #[test]
        fn repeated_tag_keeps_each_fixes_but_dedupes_see_also() {
            let issue = |info: &str| LintianIssue {
                package: None,
                package_type: Some(PackageType::Source),
                visibility: None,
                tag: Some("globbing-patterns-out-of-order".to_string()),
                info: Some(info.to_string()),
            };
            let trailers =
                render_lintian_trailers(&[issue("debian/*"), issue("src/*"), issue("tests/*")]);
            assert_eq!(
                trailers,
                "Fixes: lintian: source: globbing-patterns-out-of-order debian/*\n\
                 Fixes: lintian: source: globbing-patterns-out-of-order src/*\n\
                 Fixes: lintian: source: globbing-patterns-out-of-order tests/*\n\
                 See-also: https://lintian.debian.org/tags/globbing-patterns-out-of-order.html\n"
            );
        }

        #[test]
        fn multiple_distinct_tags_each_get_one_see_also() {
            let trailers = render_lintian_trailers(&[
                LintianIssue {
                    package: None,
                    package_type: Some(PackageType::Source),
                    visibility: None,
                    tag: Some("tag-a".to_string()),
                    info: None,
                },
                LintianIssue {
                    package: None,
                    package_type: Some(PackageType::Source),
                    visibility: None,
                    tag: Some("tag-b".to_string()),
                    info: None,
                },
            ]);
            assert_eq!(
                trailers,
                "Fixes: lintian: source: tag-a\n\
                 Fixes: lintian: source: tag-b\n\
                 See-also: https://lintian.debian.org/tags/tag-a.html\n\
                 See-also: https://lintian.debian.org/tags/tag-b.html\n"
            );
        }

        #[test]
        fn see_also_order_follows_first_occurrence_of_tag() {
            let issue = |tag: &str, info: &str| LintianIssue {
                package: None,
                package_type: Some(PackageType::Source),
                visibility: None,
                tag: Some(tag.to_string()),
                info: Some(info.to_string()),
            };
            let trailers = render_lintian_trailers(&[
                issue("tag-b", "x"),
                issue("tag-a", "y"),
                issue("tag-b", "z"),
            ]);
            let see_also: Vec<&str> = trailers
                .lines()
                .filter(|l| l.starts_with("See-also:"))
                .collect();
            assert_eq!(
                see_also,
                vec![
                    "See-also: https://lintian.debian.org/tags/tag-b.html",
                    "See-also: https://lintian.debian.org/tags/tag-a.html",
                ]
            );
        }

        #[test]
        fn binary_package_issue_includes_package_and_type() {
            let trailers = render_lintian_trailers(&[LintianIssue {
                package: Some("libfoo".to_string()),
                package_type: Some(PackageType::Binary),
                visibility: None,
                tag: Some("some-binary-tag".to_string()),
                info: Some("/usr/bin/foo".to_string()),
            }]);
            assert_eq!(
                trailers,
                "Fixes: lintian: libfoo binary: some-binary-tag /usr/bin/foo\n\
                 See-also: https://lintian.debian.org/tags/some-binary-tag.html\n"
            );
        }

        #[test]
        fn issue_without_tag_emits_fixes_but_no_see_also() {
            let trailers = render_lintian_trailers(&[LintianIssue {
                package: None,
                package_type: Some(PackageType::Source),
                visibility: None,
                tag: None,
                info: Some("orphaned".to_string()),
            }]);
            assert_eq!(trailers, "Fixes: lintian: source: orphaned\n");
        }
    }

    mod test_run_lintian_fixer {
        use super::*;

        use crate::detector::Detector;
        use crate::diagnostic::{Action, Diagnostic, FilesystemAction, RunCommandAction};

        /// Test detector that appends a line to `debian/control` and
        /// reports a fixed `some-tag` issue.
        struct DummyFixer {
            name: &'static str,
            lintian_tags: &'static [&'static str],
        }

        impl DummyFixer {
            fn new(name: &'static str, lintian_tags: &'static [&'static str]) -> Self {
                Self { name, lintian_tags }
            }
        }

        impl Detector for DummyFixer {
            fn name(&self) -> &'static str {
                self.name
            }

            fn lintian_tags(&self) -> &'static [&'static str] {
                self.lintian_tags
            }

            fn detect(
                &self,
                ws: &dyn debian_workspace::Workspace,
                _preferences: &FixerPreferences,
            ) -> Result<Vec<Diagnostic>, FixerError> {
                let control = std::path::Path::new("debian/control");
                let mut content = ws.read_file(control)?.unwrap_or_default().into_owned();
                content.extend_from_slice(b"a new line\n");
                let issue = LintianIssue {
                    package: ws.package().map(|s| s.to_string()),
                    ..LintianIssue::source("some-tag", Visibility::Warning)
                };
                Ok(vec![Diagnostic::with_actions(
                    issue,
                    "Fixed some tag.",
                    "Append a line to debian/control.",
                    vec![Action::Filesystem(FilesystemAction::Write {
                        file: control.to_path_buf(),
                        content,
                    })],
                )
                .with_certainty(Certainty::Certain)])
            }

            fn describe(
                &self,
                _fixed: &[(Diagnostic, crate::diagnostic::ActionPlan)],
                _actions: &[Action],
            ) -> String {
                "Fixed some tag.\nExtended description.".to_string()
            }
        }

        /// Test detector whose action writes some files and then fails,
        /// used to check failure bookkeeping and that the tree is
        /// reverted afterwards.
        struct FailingFixer {
            name: &'static str,
            lintian_tags: &'static [&'static str],
        }

        impl FailingFixer {
            fn new(name: &'static str, lintian_tags: &'static [&'static str]) -> Self {
                Self { name, lintian_tags }
            }
        }

        impl Detector for FailingFixer {
            fn name(&self) -> &'static str {
                self.name
            }

            fn lintian_tags(&self) -> &'static [&'static str] {
                self.lintian_tags
            }

            fn detect(
                &self,
                _ws: &dyn debian_workspace::Workspace,
                _preferences: &FixerPreferences,
            ) -> Result<Vec<Diagnostic>, FixerError> {
                // The command writes two files and then exits non-zero,
                // so applying it fails and the tree is reverted.
                Ok(vec![Diagnostic::with_actions(
                    LintianIssue::source("some-tag", Visibility::Warning),
                    "Some tag.",
                    "Run a failing command.",
                    vec![Action::RunCommand(RunCommandAction::Run {
                        argv: vec![
                            "sh".to_string(),
                            "-c".to_string(),
                            "echo blah > debian/foo; echo foo > debian/control; \
                             echo 'Not successful' >&2; exit 1"
                                .to_string(),
                        ],
                        scope: std::path::PathBuf::from("."),
                        env: vec![],
                    })],
                )])
            }
        }

        fn setup(version: Option<&str>) -> (tempfile::TempDir, GenericWorkingTree) {
            let version = version.unwrap_or("0.1");
            let td = tempfile::tempdir().unwrap();
            let tree =
                create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
            tree.mkdir(std::path::Path::new("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/control"),
                r#"Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"#,
            )
            .unwrap();
            tree.add(&[std::path::Path::new("debian/control")]).unwrap();
            std::fs::write(
                td.path().join("debian/changelog"),
                format!(
                    r#"blah ({}) UNRELEASED; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"#,
                    version
                ),
            )
            .unwrap();
            tree.add(&[std::path::Path::new("debian/changelog")])
                .unwrap();
            tree.build_commit()
                .message("Initial thingy.")
                .committer(COMMITTER)
                .commit()
                .unwrap();
            (td, tree)
        }

        #[test]
        fn test_fails() {
            let (td, tree) = setup(None);
            let lock = tree.lock_write().unwrap();
            let result = run_lintian_fixers(
                &tree,
                &[Box::new(FailingFixer::new("fail", &["some-tag"]))],
                Some(|| false),
                false,
                None,
                &FixerPreferences::default(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            std::mem::drop(lock);
            assert_eq!(0, result.success.len());
            assert_eq!(1, result.failed_fixers.len());
            let fixer = result.failed_fixers.get("fail").unwrap();
            assert!(fixer.contains("Not successful"));

            let lock = tree.lock_read().unwrap();
            assert_eq!(
                Vec::<breezyshim::tree::TreeChange>::new(),
                tree.iter_changes(&tree.basis_tree().unwrap(), None, None, None)
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            );
            std::mem::drop(lock);
            std::mem::drop(td);
        }

        #[test]
        fn test_not_debian_tree() {
            let (td, tree) = setup(None);
            tree.remove(&[(std::path::Path::new("debian/changelog"))])
                .unwrap();
            std::fs::remove_file(td.path().join("debian/changelog")).unwrap();
            tree.build_commit()
                .message("not a debian dir")
                .committer(COMMITTER)
                .commit()
                .unwrap();
            let lock = tree.lock_write().unwrap();

            assert!(matches!(
                run_lintian_fixers(
                    &tree,
                    &[Box::new(DummyFixer::new("dummy", &["some-tag"][..]))],
                    Some(|| false),
                    false,
                    None,
                    &FixerPreferences::default(),
                    None,
                    None,
                    None,
                    None,
                ),
                Err(OverallError::NotDebianPackage(_))
            ));
            std::mem::drop(lock);
            std::mem::drop(td);
        }

        #[test]
        fn test_simple_modify() {
            let (td, tree) = setup(None);
            let lock = tree.lock_write().unwrap();
            let result = run_lintian_fixers(
                &tree,
                &[Box::new(DummyFixer::new("dummy", &["some-tag"]))],
                Some(|| false),
                false,
                Some(COMMITTER),
                &FixerPreferences::default(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let revid = tree.last_revision().unwrap();
            std::mem::drop(lock);

            assert_eq!(result.success.len(), 1);
            assert_eq!(
                result.success[0].result,
                FixerResult::new(
                    "Fixed some tag.\nExtended description.".to_string(),
                    Some(Certainty::Certain),
                    None,
                    Some(revid),
                    vec![LintianIssue {
                        tag: Some("some-tag".to_string()),
                        package: Some("blah".to_string()),
                        info: None,
                        package_type: Some(PackageType::Source),
                        visibility: Some(Visibility::Warning),
                    }],
                    None,
                ),
            );
            assert_eq!(result.success[0].summary, "Fixed some tag.");
            assert_eq!(maplit::hashmap! {}, result.failed_fixers);
            assert_eq!(2, tree.branch().revno());
            let lines = tree
                .get_file_lines(std::path::Path::new("debian/control"))
                .unwrap();
            assert_eq!(lines.last().unwrap(), &b"a new line\n".to_vec());
            std::mem::drop(td);
        }

        #[test]
        fn test_below_certainty_recorded_as_uncertain() {
            struct UncertainFixer {
                name: &'static str,
                lintian_tags: &'static [&'static str],
            }

            impl Detector for UncertainFixer {
                fn name(&self) -> &'static str {
                    self.name
                }

                fn lintian_tags(&self) -> &'static [&'static str] {
                    self.lintian_tags
                }

                fn detect(
                    &self,
                    _ws: &dyn debian_workspace::Workspace,
                    _preferences: &FixerPreferences,
                ) -> Result<Vec<Diagnostic>, FixerError> {
                    Ok(vec![Diagnostic::with_actions(
                        LintianIssue::source("some-tag", Visibility::Warning),
                        "Renamed a file.",
                        "Renamed a file.",
                        vec![Action::Filesystem(FilesystemAction::Write {
                            file: std::path::PathBuf::from("debian/somefile"),
                            content: b"test".to_vec(),
                        })],
                    )
                    .with_certainty(Certainty::Possible)])
                }
            }

            let (td, tree) = setup(None);
            let lock = tree.lock_write().unwrap();
            let result = run_lintian_fixers(
                &tree,
                &[Box::new(UncertainFixer {
                    name: "dummy",
                    lintian_tags: &["some-tag"],
                })],
                Some(|| false),
                false,
                Some(COMMITTER),
                &FixerPreferences {
                    minimum_certainty: Some(Certainty::Certain),
                    ..Default::default()
                },
                None,
                None,
                None,
                None,
            )
            .unwrap();
            std::mem::drop(lock);

            assert_eq!(0, result.success.len());
            assert_eq!(maplit::hashmap! {}, result.failed_fixers);
            assert_eq!(
                maplit::hashmap! { "dummy".to_string() => Certainty::Possible },
                result.uncertain_fixers
            );
            // No commit was made.
            assert_eq!(1, tree.branch().revno());
            std::mem::drop(td);
        }

        #[test]
        fn test_changelog_entry_has_asterisk_prefix() {
            let (td, tree) = setup(None);
            let lock = tree.lock_write().unwrap();
            let _result = run_lintian_fixers(
                &tree,
                &[Box::new(DummyFixer::new("dummy", &["some-tag"]))],
                Some(|| true), // Update changelog
                false,
                Some(COMMITTER),
                &FixerPreferences::default(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            std::mem::drop(lock);

            // Read the changelog and verify that entries start with "* "
            let changelog_content =
                std::fs::read_to_string(td.path().join("debian/changelog")).unwrap();
            let changelog: ChangeLog = changelog_content.parse().unwrap();
            let first_entry = changelog.iter().next().unwrap();
            let change_lines: Vec<String> = first_entry.change_lines().collect();

            // Filter out author section headers (lines starting with "[") and empty lines
            let bullet_lines: Vec<String> = change_lines
                .iter()
                .filter(|line| line.starts_with("* "))
                .cloned()
                .collect();

            // Should have exactly 3 lines: our 2 new entries + the original one
            assert_eq!(bullet_lines.len(), 3);

            // Verify the exact entries - original is first, then our new entries
            assert_eq!(bullet_lines[0], "* Initial release. (Closes: #911016)");
            assert_eq!(bullet_lines[1], "* Fixed some tag.");
            assert_eq!(bullet_lines[2], "* Extended description.");

            std::mem::drop(td);
        }

        #[test]
        fn test_simple_modify_too_uncertain() {
            let (td, tree) = setup(None);

            struct UncertainFixer {
                name: &'static str,
                lintian_tags: &'static [&'static str],
            }

            impl UncertainFixer {
                fn new(name: &'static str, lintian_tags: &'static [&'static str]) -> Self {
                    Self { name, lintian_tags }
                }
            }

            impl Detector for UncertainFixer {
                fn name(&self) -> &'static str {
                    self.name
                }

                fn lintian_tags(&self) -> &'static [&'static str] {
                    self.lintian_tags
                }

                fn detect(
                    &self,
                    _ws: &dyn debian_workspace::Workspace,
                    _preferences: &FixerPreferences,
                ) -> Result<Vec<Diagnostic>, FixerError> {
                    Ok(vec![Diagnostic::with_actions(
                        LintianIssue::source("some-tag", Visibility::Warning),
                        "Renamed a file.",
                        "Renamed a file.",
                        vec![Action::Filesystem(FilesystemAction::Write {
                            file: std::path::PathBuf::from("debian/somefile"),
                            content: b"test".to_vec(),
                        })],
                    )
                    .with_certainty(Certainty::Possible)])
                }
            }

            let lock_write = tree.lock_write().unwrap();

            let result = run_lintian_fixer(
                &tree,
                &UncertainFixer::new("dummy", &["some-tag"]),
                Some(COMMITTER),
                || false,
                &FixerPreferences {
                    minimum_certainty: Some(Certainty::Certain),
                    ..Default::default()
                },
                &mut None,
                Path::new(""),
                None,
                None,
                None,
            );

            assert!(
                matches!(result, Err(FixerError::NotCertainEnough(..))),
                "{:?}",
                result
            );
            assert_eq!(1, tree.branch().revno());
            std::mem::drop(lock_write);
            std::mem::drop(td);
        }

        #[test]
        fn test_simple_modify_acceptably_uncertain() {
            let (td, tree) = setup(None);

            struct UncertainFixer {
                name: &'static str,
                lintian_tags: &'static [&'static str],
            }

            impl UncertainFixer {
                fn new(name: &'static str, lintian_tags: &'static [&'static str]) -> Self {
                    Self { name, lintian_tags }
                }
            }

            impl Detector for UncertainFixer {
                fn name(&self) -> &'static str {
                    self.name
                }

                fn lintian_tags(&self) -> &'static [&'static str] {
                    self.lintian_tags
                }

                fn detect(
                    &self,
                    _ws: &dyn debian_workspace::Workspace,
                    _preferences: &FixerPreferences,
                ) -> Result<Vec<Diagnostic>, FixerError> {
                    Ok(vec![Diagnostic::with_actions(
                        LintianIssue::source("some-tag", Visibility::Warning),
                        "Renamed a file.",
                        "Renamed a file.",
                        vec![Action::Filesystem(FilesystemAction::Write {
                            file: std::path::PathBuf::from("debian/somefile"),
                            content: b"test".to_vec(),
                        })],
                    )
                    .with_certainty(Certainty::Possible)])
                }
            }

            let lock_write = tree.lock_write().unwrap();

            let (_result, summary) = run_lintian_fixer(
                &tree,
                &UncertainFixer::new("dummy", &["some-tag"]),
                Some("Testsuite <lintian-brush@example.com>"),
                || false,
                &FixerPreferences {
                    minimum_certainty: Some(Certainty::Possible),
                    ..Default::default()
                },
                &mut None,
                Path::new(""),
                None,
                None,
                None,
            )
            .unwrap();

            assert_eq!("Renamed a file.", summary);

            assert_eq!(2, tree.branch().revno());

            std::mem::drop(lock_write);
            std::mem::drop(td);
        }

        #[test]
        fn test_new_file() {
            let (td, tree) = setup(None);

            struct NewFileFixer {
                name: &'static str,
                lintian_tags: &'static [&'static str],
            }

            impl NewFileFixer {
                fn new(name: &'static str, lintian_tags: &'static [&'static str]) -> Self {
                    Self { name, lintian_tags }
                }
            }

            impl Detector for NewFileFixer {
                fn name(&self) -> &'static str {
                    self.name
                }

                fn lintian_tags(&self) -> &'static [&'static str] {
                    self.lintian_tags
                }

                fn detect(
                    &self,
                    ws: &dyn debian_workspace::Workspace,
                    _preferences: &FixerPreferences,
                ) -> Result<Vec<Diagnostic>, FixerError> {
                    let issue = LintianIssue {
                        package: ws.package().map(|s| s.to_string()),
                        ..LintianIssue::source("some-tag", Visibility::Warning)
                    };
                    Ok(vec![Diagnostic::with_actions(
                        issue,
                        "Created new file.",
                        "Created new file.",
                        vec![Action::Filesystem(FilesystemAction::Write {
                            file: std::path::PathBuf::from("debian/somefile"),
                            content: b"test".to_vec(),
                        })],
                    )])
                }
            }

            let lock_write = tree.lock_write().unwrap();

            let (result, summary) = run_lintian_fixer(
                &tree,
                &NewFileFixer::new("new-file", &["some-tag"]),
                Some(COMMITTER),
                || false,
                &FixerPreferences::default(),
                &mut None,
                Path::new(""),
                None,
                None,
                None,
            )
            .unwrap();

            assert_eq!("Created new file.", summary);
            assert_eq!(result.certainty, None);
            assert_eq!(result.fixed_lintian_tags(), &["some-tag"]);
            let rev = tree
                .branch()
                .repository()
                .get_revision(&tree.last_revision().unwrap())
                .unwrap();
            assert_eq!(
                rev.message,
                "Created new file.\n\nChanges-By: lintian-brush\nFixes: lintian: blah source: some-tag\nSee-also: https://lintian.debian.org/tags/some-tag.html\n"
            );
            assert_eq!(2, tree.branch().revno());
            let basis_tree = tree.branch().basis_tree().unwrap();
            let basis_lock = basis_tree.lock_read().unwrap();
            assert_eq!(
                basis_tree
                    .get_file_text(Path::new("debian/somefile"))
                    .unwrap(),
                b"test"
            );
            std::mem::drop(basis_lock);
            std::mem::drop(lock_write);
            std::mem::drop(td);
        }

        #[test]
        fn test_rename_file() {
            let (td, tree) = setup(None);

            struct RenameFileFixer {
                name: &'static str,
                lintian_tags: &'static [&'static str],
            }

            impl RenameFileFixer {
                fn new(name: &'static str, lintian_tags: &'static [&'static str]) -> Self {
                    Self { name, lintian_tags }
                }
            }

            impl Detector for RenameFileFixer {
                fn name(&self) -> &'static str {
                    self.name
                }

                fn lintian_tags(&self) -> &'static [&'static str] {
                    self.lintian_tags
                }

                fn detect(
                    &self,
                    _ws: &dyn debian_workspace::Workspace,
                    _preferences: &FixerPreferences,
                ) -> Result<Vec<Diagnostic>, FixerError> {
                    Ok(vec![Diagnostic::with_actions(
                        LintianIssue::source("some-tag", Visibility::Warning),
                        "Renamed a file.",
                        "Renamed a file.",
                        vec![Action::Filesystem(FilesystemAction::Rename {
                            file: std::path::PathBuf::from("debian/control"),
                            to: std::path::PathBuf::from("debian/control.blah"),
                        })],
                    )])
                }
            }

            let orig_basis_tree = tree.branch().basis_tree().unwrap();
            let lock_write = tree.lock_write().unwrap();
            let (result, summary) = run_lintian_fixer(
                &tree,
                &RenameFileFixer::new("rename", &["some-tag"]),
                Some(COMMITTER),
                || false,
                &FixerPreferences::default(),
                &mut None,
                Path::new(""),
                None,
                None,
                None,
            )
            .unwrap();
            assert_eq!("Renamed a file.", summary);
            assert_eq!(result.certainty, None);
            assert_eq!(2, tree.branch().revno());
            let basis_tree = tree.branch().basis_tree().unwrap();
            let basis_lock = basis_tree.lock_read().unwrap();
            let orig_basis_tree_lock = orig_basis_tree.lock_read().unwrap();
            assert!(!basis_tree.has_filename(Path::new("debian/control")));
            assert!(basis_tree.has_filename(Path::new("debian/control.blah")));
            assert_ne!(
                orig_basis_tree.get_revision_id(),
                basis_tree.get_revision_id()
            );
            std::mem::drop(orig_basis_tree_lock);
            std::mem::drop(basis_lock);
            std::mem::drop(lock_write);
            std::mem::drop(td);
        }

        #[test]
        fn test_empty_change() {
            let (td, tree) = setup(None);

            /// Detector that finds nothing to fix.
            struct EmptyFixer {
                name: &'static str,
                lintian_tags: &'static [&'static str],
            }

            impl EmptyFixer {
                fn new(name: &'static str, lintian_tags: &'static [&'static str]) -> Self {
                    Self { name, lintian_tags }
                }
            }

            impl Detector for EmptyFixer {
                fn name(&self) -> &'static str {
                    self.name
                }

                fn lintian_tags(&self) -> &'static [&'static str] {
                    self.lintian_tags
                }

                fn detect(
                    &self,
                    _ws: &dyn debian_workspace::Workspace,
                    _preferences: &FixerPreferences,
                ) -> Result<Vec<Diagnostic>, FixerError> {
                    Ok(vec![])
                }
            }

            let lock_write = tree.lock_write().unwrap();

            let result = run_lintian_fixer(
                &tree,
                &EmptyFixer::new("empty", &["some-tag"]),
                Some(COMMITTER),
                || false,
                &FixerPreferences::default(),
                &mut None,
                Path::new(""),
                None,
                None,
                None,
            );

            assert!(matches!(result, Err(FixerError::NoChanges)), "{:?}", result);
            assert_eq!(1, tree.branch().revno());

            assert_eq!(
                Vec::<breezyshim::tree::TreeChange>::new(),
                tree.iter_changes(&tree.basis_tree().unwrap(), None, None, None)
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            );

            std::mem::drop(lock_write);

            std::mem::drop(td);
        }

        #[test]
        fn test_upstream_change() {
            let (td, tree) = setup(Some("0.1-1"));

            #[derive(Debug)]
            struct NewFileFixer {
                name: &'static str,
                lintian_tags: &'static [&'static str],
            }

            impl NewFileFixer {
                fn new(name: &'static str, lintian_tags: &'static [&'static str]) -> Self {
                    Self { name, lintian_tags }
                }
            }

            impl Detector for NewFileFixer {
                fn name(&self) -> &'static str {
                    self.name
                }

                fn lintian_tags(&self) -> &'static [&'static str] {
                    self.lintian_tags
                }

                fn detect(
                    &self,
                    _ws: &dyn debian_workspace::Workspace,
                    _preferences: &FixerPreferences,
                ) -> Result<Vec<Diagnostic>, FixerError> {
                    Ok(vec![Diagnostic::untagged(
                        "Created new configure.ac.",
                        "Created new configure.ac.",
                        vec![Action::Filesystem(FilesystemAction::Write {
                            file: std::path::PathBuf::from("configure.ac"),
                            content: b"AC_INIT(foo, bar)\n".to_vec(),
                        })],
                    )
                    .with_patch_name("add-config")])
                }
            }

            let lock = tree.lock_write().unwrap();

            let (result, summary) = run_lintian_fixer(
                &tree,
                &NewFileFixer::new("add-config", &["add-config"]),
                Some(COMMITTER),
                || false,
                &FixerPreferences::default(),
                &mut None,
                Path::new(""),
                Some(
                    chrono::DateTime::parse_from_rfc3339("2020-09-08T00:36:35Z")
                        .unwrap()
                        .naive_utc(),
                ),
                None,
                None,
            )
            .unwrap();
            assert_eq!(
                summary,
                "Add patch add-config.patch: Created new configure.ac."
            );
            assert_eq!(result.certainty, None);
            let rev = tree
                .branch()
                .repository()
                .get_revision(&tree.last_revision().unwrap())
                .unwrap();
            assert_eq!(
                rev.message,
                "Created new configure.ac.\n\nChanges-By: lintian-brush\n"
            );
            assert_eq!(2, tree.branch().revno());
            let basis_tree = tree.branch().basis_tree().unwrap();
            let basis_lock = basis_tree.lock_read().unwrap();
            assert_eq!(
                basis_tree
                    .get_file_text(Path::new("debian/patches/series"))
                    .unwrap(),
                b"add-config.patch\n"
            );
            let lines = basis_tree
                .get_file_lines(Path::new("debian/patches/add-config.patch"))
                .unwrap();
            assert_eq!(lines[0], b"Description: Created new configure.ac.\n");
            assert_eq!(lines[1], b"Origin: other\n");
            assert_eq!(lines[2], b"Last-Update: 2020-09-08\n");
            assert_eq!(lines[3], b"---\n");
            assert_eq!(lines[4], b"=== added file 'configure.ac'\n");
            assert_eq!(
                &lines[5][..(b"--- a/configure.ac".len())],
                b"--- a/configure.ac"
            );
            assert_eq!(
                &lines[6][..(b"+++ b/configure.ac".len())],
                b"+++ b/configure.ac"
            );
            assert_eq!(lines[7], b"@@ -0,0 +1,1 @@\n");
            assert_eq!(lines[8], b"+AC_INIT(foo, bar)\n");

            std::mem::drop(basis_lock);
            std::mem::drop(lock);
            std::mem::drop(td);
        }

        #[test]
        fn test_upstream_change_stacked() {
            let (td, tree) = setup(Some("0.1-1"));

            std::fs::create_dir(td.path().join("debian/patches")).unwrap();
            std::fs::write(td.path().join("debian/patches/series"), "foo\n").unwrap();
            std::fs::write(
                td.path().join("debian/patches/foo"),
                r###"--- /dev/null	2020-09-07 13:26:27.546468905 +0000
+++ a	2020-09-08 01:26:25.811742671 +0000
@@ -0,0 +1 @@
+foo
"###,
            )
            .unwrap();
            tree.add(&[
                Path::new("debian/patches"),
                Path::new("debian/patches/series"),
                Path::new("debian/patches/foo"),
            ])
            .unwrap();
            tree.build_commit()
                .committer(COMMITTER)
                .message("Add patches")
                .commit()
                .unwrap();

            struct NewFileFixer {
                name: &'static str,
                lintian_tags: &'static [&'static str],
            }

            impl NewFileFixer {
                fn new(name: &'static str, lintian_tags: &'static [&'static str]) -> Self {
                    Self { name, lintian_tags }
                }
            }

            impl Detector for NewFileFixer {
                fn name(&self) -> &'static str {
                    self.name
                }

                fn lintian_tags(&self) -> &'static [&'static str] {
                    self.lintian_tags
                }

                fn detect(
                    &self,
                    _ws: &dyn debian_workspace::Workspace,
                    _preferences: &FixerPreferences,
                ) -> Result<Vec<Diagnostic>, FixerError> {
                    Ok(vec![Diagnostic::untagged(
                        "Created new configure.ac.",
                        "Created new configure.ac.",
                        vec![Action::Filesystem(FilesystemAction::Write {
                            file: std::path::PathBuf::from("configure.ac"),
                            content: b"AC_INIT(foo, bar)\n".to_vec(),
                        })],
                    )
                    .with_patch_name("add-config")])
                }
            }

            let lock = tree.lock_write().unwrap();

            let result = run_lintian_fixer(
                &tree,
                &NewFileFixer::new("add-config", &["add-config"]),
                Some(COMMITTER),
                || false,
                &FixerPreferences::default(),
                &mut None,
                Path::new(""),
                Some(
                    chrono::DateTime::parse_from_rfc3339("2020-09-08T00:36:35Z")
                        .unwrap()
                        .naive_utc(),
                ),
                None,
                None,
            );

            std::mem::drop(lock);

            assert!(matches!(
                result,
                Err(FixerError::FailedPatchManipulation(..))
            ));
            std::mem::drop(td);
        }

        fn make_package_tree(path: &Path, format: &str) -> GenericWorkingTree {
            let tree = create_standalone_workingtree(path, format).unwrap();
            std::fs::create_dir(path.join("debian")).unwrap();
            std::fs::write(
                path.join("debian/control"),
                r#""Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"#,
            )
            .unwrap();
            std::fs::write(
                path.join("debian/changelog"),
                r#"blah (0.1-1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"#,
            )
            .unwrap();
            tree.add(&[
                Path::new("debian"),
                Path::new("debian/changelog"),
                Path::new("debian/control"),
            ])
            .unwrap();
            tree.build_commit()
                .committer(COMMITTER)
                .message("Initial thingy.")
                .commit()
                .unwrap();
            tree
        }

        fn make_change(tree: &GenericWorkingTree, committer: Option<&str>) {
            let lock = tree.lock_write().unwrap();

            let (result, summary) = run_lintian_fixer(
                tree,
                &DummyFixer::new("dummy", &["some-tag"]),
                committer,
                || false,
                &FixerPreferences::default(),
                &mut None,
                Path::new(""),
                None,
                None,
                None,
            )
            .unwrap();
            assert_eq!(summary, "Fixed some tag.");
            assert_eq!(vec!["some-tag"], result.fixed_lintian_tags());
            assert_eq!(Some(Certainty::Certain), result.certainty);
            assert_eq!(2, tree.branch().revno());
            let lines = tree.get_file_lines(Path::new("debian/control")).unwrap();
            assert_eq!(lines.last().unwrap(), b"a new line\n");
            std::mem::drop(lock);
        }

        #[test]
        fn test_honors_tree_committer_specified() {
            let td = tempfile::tempdir().unwrap();
            let tree = make_package_tree(td.path(), "git");

            make_change(&tree, Some("Jane Example <jane@example.com>"));

            let rev = tree
                .branch()
                .repository()
                .get_revision(&tree.branch().last_revision())
                .unwrap();
            assert_eq!(rev.committer, "Jane Example <jane@example.com>");
        }

        #[test]
        fn test_honors_tree_committer_config() {
            let td = tempfile::tempdir().unwrap();
            let tree = make_package_tree(td.path(), "git");
            std::fs::write(
                td.path().join(".git/config"),
                r###"
[user]
  email = jane@example.com
  name = Jane Example
"###,
            )
            .unwrap();

            make_change(&tree, None);

            let rev = tree
                .branch()
                .repository()
                .get_revision(&tree.branch().last_revision())
                .unwrap();
            assert_eq!(rev.committer, "Jane Example <jane@example.com>");
        }
    }

    mod many_result_tests {
        use super::*;

        #[test]
        fn test_empty() {
            let result = ManyResult::default();
            assert_eq!(Certainty::Certain, result.minimum_success_certainty());
        }

        #[test]
        fn test_no_certainty() {
            let mut result = ManyResult::default();
            result.success.push(FixerSuccess {
                result: FixerResult::new(
                    "Do bla".to_string(),
                    None,
                    None,
                    None,
                    vec![LintianIssue::just_tag("tag-a".to_string())],
                    None,
                ),
                summary: "summary".to_string(),
                fixer_name: "test-fixer".to_string(),
            });
            assert_eq!(Certainty::Certain, result.minimum_success_certainty());
        }

        #[test]
        fn test_possible() {
            let mut result = ManyResult::default();
            result.success.push(FixerSuccess {
                result: FixerResult::new(
                    "Do bla".to_string(),
                    Some(Certainty::Possible),
                    None,
                    None,
                    vec![LintianIssue::just_tag("tag-a".to_string())],
                    None,
                ),
                summary: "summary".to_string(),
                fixer_name: "test-fixer-1".to_string(),
            });
            result.success.push(FixerSuccess {
                result: FixerResult::new(
                    "Do bloeh".to_string(),
                    Some(Certainty::Certain),
                    None,
                    None,
                    vec![LintianIssue::just_tag("tag-b".to_string())],
                    None,
                ),
                summary: "summary".to_string(),
                fixer_name: "test-fixer-2".to_string(),
            });
            assert_eq!(Certainty::Possible, result.minimum_success_certainty());
        }
    }
}

#[cfg(test)]
mod fixer_tests;

#[cfg(test)]
mod fixer_result_builder_tests {
    use super::*;

    #[test]
    fn test_fixer_result_builder_basic() {
        let result = FixerResult::builder("Test fix").build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.certainty, None);
        assert_eq!(result.patch_name, None);
        assert_eq!(result.revision_id, None);
        assert_eq!(result.fixed_lintian_issues.len(), 0);
        assert_eq!(result.overridden_lintian_issues.len(), 0);
    }

    #[test]
    fn test_fixer_result_builder_with_certainty() {
        let result = FixerResult::builder("Test fix")
            .certainty(Certainty::Confident)
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.certainty, Some(Certainty::Confident));
    }

    #[test]
    fn test_fixer_result_builder_with_patch_name() {
        let result = FixerResult::builder("Test fix")
            .patch_name("test.patch")
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.patch_name, Some("test.patch".to_string()));
    }

    #[test]
    fn test_fixer_result_builder_with_fixed_tags() {
        let result = FixerResult::builder("Test fix")
            .fixed_issue(LintianIssue::just_tag("tag1".to_string()))
            .fixed_issue(LintianIssue::just_tag("tag2".to_string()))
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.fixed_lintian_tags(), vec!["tag1", "tag2"]);
    }

    #[test]
    fn test_fixer_result_builder_with_fixed_tags_batch() {
        let result = FixerResult::builder("Test fix")
            .fixed_issues([
                LintianIssue::just_tag("tag1".to_string()),
                LintianIssue::just_tag("tag2".to_string()),
                LintianIssue::just_tag("tag3".to_string()),
            ])
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.fixed_lintian_tags(), vec!["tag1", "tag2", "tag3"]);
    }

    #[test]
    fn test_fixer_result_builder_with_fixed_issues() {
        let issue1 = LintianIssue::just_tag("tag1".to_string());
        let issue2 = LintianIssue::just_tag("tag2".to_string());

        let result = FixerResult::builder("Test fix")
            .fixed_issue(issue1)
            .fixed_issue(issue2)
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.fixed_lintian_tags(), vec!["tag1", "tag2"]);
    }

    #[test]
    fn test_fixer_result_builder_with_fixed_issues_batch() {
        let issues = vec![
            LintianIssue::just_tag("tag1".to_string()),
            LintianIssue::just_tag("tag2".to_string()),
        ];

        let result = FixerResult::builder("Test fix")
            .fixed_issues(issues)
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.fixed_lintian_tags(), vec!["tag1", "tag2"]);
    }

    #[test]
    fn test_fixer_result_builder_with_overridden_issues() {
        let issue = LintianIssue::just_tag("overridden-tag".to_string());

        let result = FixerResult::builder("Test fix")
            .overridden_issue(issue)
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.overridden_lintian_issues.len(), 1);
        assert_eq!(
            result.overridden_lintian_issues[0].tag,
            Some("overridden-tag".to_string())
        );
    }

    #[test]
    fn test_fixer_result_builder_with_overridden_issues_batch() {
        let issues = vec![
            LintianIssue::just_tag("tag1".to_string()),
            LintianIssue::just_tag("tag2".to_string()),
        ];

        let result = FixerResult::builder("Test fix")
            .overridden_issues(issues)
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.overridden_lintian_issues.len(), 2);
    }

    #[test]
    fn test_fixer_result_builder_chain_all() {
        let revision_id = breezyshim::RevisionId::null(); // Use null for testing

        let result = FixerResult::builder("Test fix")
            .certainty(Certainty::Certain)
            .patch_name("comprehensive.patch")
            .revision_id(revision_id.clone())
            .fixed_issue(LintianIssue::just_tag("fixed-tag".to_string()))
            .overridden_issue(LintianIssue::just_tag("overridden-tag".to_string()))
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(result.patch_name, Some("comprehensive.patch".to_string()));
        assert_eq!(result.revision_id, Some(revision_id));
        assert_eq!(result.fixed_lintian_tags(), vec!["fixed-tag"]);
        assert_eq!(result.overridden_lintian_issues.len(), 1);
    }

    #[test]
    fn test_fixer_result_builder_mixed_tags_and_issues() {
        let issue = LintianIssue::just_tag("issue-tag".to_string());

        let result = FixerResult::builder("Test fix")
            .fixed_issue(LintianIssue::just_tag("tag1".to_string()))
            .fixed_issue(issue)
            .fixed_issue(LintianIssue::just_tag("tag2".to_string()))
            .build();

        let tags = result.fixed_lintian_tags();
        assert_eq!(tags.len(), 3);
        assert!(tags.contains(&"tag1"));
        assert!(tags.contains(&"tag2"));
        assert!(tags.contains(&"issue-tag"));
    }
}
