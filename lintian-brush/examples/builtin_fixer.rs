use lintian_brush::{declare_fixer, Certainty, FixerError, FixerResult};

// Example of a simple builtin fixer
declare_fixer! {
    name: "example-fixer",
    tags: ["example-tag"],
    apply: |_basedir, _package, _version, _preferences| {
        // This is just an example - it doesn't actually do anything
        Ok(FixerResult::builder("Fixed example issue")
            .certainty(Certainty::Certain)
            .fixed_tag("example-tag")
            .build())
    }
}

fn main() {
    println!("This is an example of how to create a builtin fixer");
}