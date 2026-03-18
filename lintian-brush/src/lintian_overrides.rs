//! Lintian overrides parsing and manipulation.
//!
//! This module re-exports everything from the `lintian-overrides` crate and adds
//! an extension trait for matching override lines against [`LintianIssue`](crate::LintianIssue)s.

pub use lintian_overrides::*;

/// Extension trait for matching override lines against LintianIssue
pub trait OverrideLineMatch {
    /// Check if this override matches a LintianIssue
    fn matches_issue(&self, issue: &crate::LintianIssue) -> bool;
}

impl OverrideLineMatch for OverrideLine {
    fn matches_issue(&self, issue: &crate::LintianIssue) -> bool {
        self.matches(
            issue.tag.as_deref(),
            issue.package.as_deref(),
            issue
                .package_type
                .as_ref()
                .map(|t| t.to_string())
                .as_deref(),
            issue.info.as_deref(),
        )
    }
}
