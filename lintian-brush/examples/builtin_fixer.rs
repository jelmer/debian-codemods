use lintian_brush::diagnostic::Diagnostic;
use lintian_brush::{declare_fixer, Certainty, LintianIssue};

declare_fixer! {
    name: "example-fixer",
    tags: ["example-tag"],
    diagnose: |_basedir, _package, _version, _preferences| {
        Ok(vec![Diagnostic::with_actions(
            LintianIssue::source("example-tag"),
            "Fixed example issue",
            Vec::new(),
        )
        .with_certainty(Certainty::Certain)])
    }
}

fn main() {
    println!("This is an example of how to create a builtin fixer");
}
