This directory contains the fixers for lintian-brush in rust.

Each fixer is registered as a `Detector`. A detector reads a Debian
source package through a `Workspace` and emits `Diagnostic`s
describing what (if anything) needs fixing, together with the
`Action`s that would fix it. Detectors do *not* mutate the tree —
applying actions is the runtime's job.

The common pattern:

```rust
pub fn detect(
    ws: &dyn Workspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    // ... read files via ws, build diagnostics ...
}

declare_detector! {
    name: "fixer-name",
    tags: ["tag1", "tag2"],
    detect: |ws, prefs| detect(ws, prefs),
}
```

Optional clauses on `declare_detector!`:

- `after: ["other-fixer-name", ...]` and/or `before: [...]` — ordering
  constraints relative to other detectors.
- `describe: |fixed, actions| { ... }` — override the default commit
  message generator.

The detector's job is to:

1. Build a `LintianIssue` matching how `lintian` would report the
   problem (including the info field, byte-for-byte).
2. Pair it with one or more `ActionPlan`s that would fix it.

The runtime then:

- Calls `LintianIssue::should_fix()` to honour lintian overrides.
- Filters diagnostics below `preferences.minimum_certainty`.
- Picks the first `ActionPlan` whose `opinionated` flag is satisfied
  by `preferences.opinionated`.
- Applies the actions via `crate::appliers::apply_actions`.
- Returns a `FixerResult` listing the fixed and overridden issues.

Fixers may panic — `detector::detect_and_fix` catches the panic and reports
it as `FixerError::Panic`.

Each fixer should have some unit tests for its logic. In addition, it
should have some integration tests in the
`lintian-brush/tests/<fixer_name>` directory.

Ideally a fixer comes with some ActionPlans that resolve it,
but it's also fine if it just reports the issue without a fix.
