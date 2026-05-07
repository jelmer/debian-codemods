//! Minimal example of declaring a builtin fixer via [`declare_detector!`].
//!
//! Detectors read the package through a [`FixerWorkspace`] and emit
//! [`Diagnostic`]s describing what needs fixing. The runtime applies the
//! associated actions and produces the resulting commit.

use lintian_brush::declare_detector;
use lintian_brush::diagnostic::Diagnostic;
use lintian_brush::workspace::FixerWorkspace;
use lintian_brush::{Certainty, FixerError, FixerPreferences, LintianIssue};

fn detect(
    _ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    Ok(vec![Diagnostic::with_actions(
        LintianIssue::source("example-tag"),
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
