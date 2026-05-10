//! Minimal example of declaring a builtin fixer via [`declare_detector!`].
//!
//! Detectors read the package through a [`Workspace`] and emit
//! [`Diagnostic`]s describing what needs fixing. The runtime applies the
//! associated actions and produces the resulting commit.

use lintian_brush::declare_detector;
use lintian_brush::diagnostic::Diagnostic;
use lintian_brush::workspace::Workspace;
use lintian_brush::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};

fn detect(
    _ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    Ok(vec![Diagnostic::with_actions(
        LintianIssue::source("example-tag", Visibility::Warning),
        "Example issue is present.",
        "Fix example issue.",
        Vec::new(),
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "example-fixer",
    tags: ["example-tag"],
    detect: |ws, prefs| detect(ws, prefs),
}

fn main() {
    println!("This is an example of how to create a builtin fixer");
}
