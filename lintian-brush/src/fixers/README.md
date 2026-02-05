This directory contains the fixers for lintian-brush in rust.

You can see there is a common pattern:

```rust
declare_fixer! {
   name: "fixer_name",  // Name of the fixer 
   tags: ["tag1", "tag2"],   /// Tags associated with the fixer
   apply: |basedir, package, version, preferences| {
     // Logic to apply the fixer
   }
}
```

Each fixer is declared using the `declare_fixer!` macro, which takes the following parameters:

 - `basedir`: The base directory where the package is located
 - `package`: The name of the source package to be fixed
 - `version`: The current version of the package
 - `preferences`: User preferences that may influence the fixing process

Fixers are meant to detect issues in the package. For each, they create a ``LintianIssue``
object that describes the issue that matches how `lintian` would report it
(this includes matching the info field byte-for-byte).
After that, they call `issue.should_fix()` to check if the user wants to fix
the issue (the main reason this will return false is when there is a lintian
override in place).

If `should_fix()` returns true, then the fixer applies the necessary changes. Finally,
the fixer returns a `FixerResult` indicating what sort of changes were made. This
can be `Err(FixerError::NoChanges)`, `Ok(FixerResult::Fixed)` or `Err(FixerError::Other)`. The successful result includes a list of issues that were fixed and a list of
issues that were detected but not fixed.

Fixers can panic - the caller will catch the panic and report it as an error.

Each fixer should have some unit tests for its logic. In addition, it should have
some integration tests in the `lintian-brush/tests/<fixer_name>` directory.
