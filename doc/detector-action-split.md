# Splitting detectors from actions

## Background

Today, every lintian-brush fixer (see `lintian-brush/src/fixers/*.rs`) is a
single function that both *detects* an issue and *modifies* the tree to fix
it. The two concerns are interleaved: a fixer opens the relevant control file,
walks it looking for problems, and as it finds them mutates the file in place.

This made sense when the only consumer was the batch driver
(`run_lintian_fixers` in `lintian-brush/src/lib.rs`), which simply runs every
fixer top-to-bottom and commits whatever they produce. It does not work well
for the directions we want to head in:

* **LSP / editor integration.** A user wants to see diagnostics first and pick
  fixes one by one, possibly in an order that has nothing to do with the order
  fixers happen to be registered in. The current shape forces re-running the
  whole detection pass to surface a fix.
* **Reuse across detector and fixer.** A planned "diagnostics only" mode (the
  motivation for this branch) would have to re-implement most of the detection
  logic from each fixer, since detection is not exposed as a separate
  operation. The two implementations would inevitably drift.
* **Override handling.** Each fixer calls `LintianIssue::should_fix` itself
  and returns `FixerError::NoChangesAfterOverrides` on its own. The control
  framework cannot make a global decision about overrides without trusting
  every fixer to consult them correctly.
* **Multiple fixes per issue.** A single diagnostic may legitimately have
  several reasonable fixes (e.g. *bump Standards-Version to 4.7.0* vs *bump
  to 4.7.2*). The current API has no way to express that.

## Proposal

Split each fixer into two pieces:

1. A **detector** that reads the tree and returns a list of
   `Diagnostic { issue, actions: [...] }` records. Detectors do **not** mutate
   the tree.
2. A small set of generic **action appliers** that know how to apply a given
   action variant. Detectors emit actions that carry enough context for the
   applier to perform the fix without re-reading or re-analysing anything.

Roughly:

```rust
pub trait Detector: Send + Sync {
    fn name(&self) -> &'static str;
    fn lintian_tags(&self) -> &'static [&'static str];

    fn detect(
        &self,
        ctx: &DetectionContext,
    ) -> Result<Vec<Diagnostic>, DetectorError>;
}

pub struct Diagnostic {
    pub issue: LintianIssue,
    pub certainty: Certainty,
    pub actions: Vec<Action>,
    /// Human-readable summary, used for the commit message / LSP message.
    pub message: String,
}

/// Outer enum is dispatched on file kind. Each variant carries a
/// per-file-type action enum so the deb822 applier never has to know
/// about makefiles and vice versa.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum Action {
    Deb822(Deb822Action),
    Makefile(MakefileAction),
    Changelog(ChangelogAction),
    Watch(WatchAction),
    Filesystem(FilesystemAction),
    // ...one variant per file family we touch.
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op")]
pub enum Deb822Action {
    SetField    { file: PathBuf, paragraph: ParagraphSelector,
                   field: String, value: String },
    RemoveField { file: PathBuf, paragraph: ParagraphSelector,
                   field: String },
    RenameField { file: PathBuf, paragraph: ParagraphSelector,
                   from: String, to: String },
    InsertParagraph { file: PathBuf, after: ParagraphSelector,
                       fields: Vec<(String, String)> },
    RemoveParagraph { file: PathBuf, paragraph: ParagraphSelector },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op")]
pub enum FilesystemAction {
    SetMode    { file: PathBuf, mode: u32 },
    Delete     { file: PathBuf },
    Write      { file: PathBuf, content: Vec<u8> },
    ReplaceText{ file: PathBuf, range: TextRange, replacement: String },
}

// MakefileAction, ChangelogAction, WatchAction follow the same shape:
// one enum per file family, each variant tagged for serde.
```

Each action variant is self-describing. `Deb822Action::SetField` for example
carries the file path, a paragraph selector (e.g. `Source` or `Binary("foo")`),
a field name and a value — that is all an applier needs. The applier does
not need to know *why* the field is being set, only how to set it.

The per-file-type split keeps the deb822 applier from having to match on
makefile variants, lets each file family evolve its own action vocabulary
independently, and means a wire payload that arrives over LSP and fails to
deserialise as `Deb822Action` is rejected before it reaches the deb822
applier.

## What this changes for an example fixer

`lintian-brush/src/fixers/no_priority_field.rs` today opens
`debian/control`, walks the binaries, decides whether `Priority: optional` is
the new dpkg default, and either writes a value into the source paragraph or
into each binary. A diagnostic-only run has to redo all of that.

After the split, the detector returns:

```text
Diagnostic {
    issue:   "recommended-field" on source,
    message: "Priority: optional missing on binary 'foo'",
    actions: [Action::Deb822(Deb822Action::SetField {
        file: "debian/control",
        paragraph: Binary("foo"),
        field: "Priority",
        value: "optional",
    })],
}
```

…or, in the "promote shared priority to source" case, several
`Deb822Action::RemoveField` actions plus one `Deb822Action::SetField` on the
source — all attached to a single diagnostic.

The applier knows how to apply each `Deb822Action`. There is exactly one
implementation of each variant, shared by every detector that emits one.
Rerunning the detector after applying actions is also no longer needed: an
LSP client can present each action as its own quickfix and apply them in any
order.

## Override handling

`LintianIssue::should_fix` moves out of the individual fixers and into the
driver. The flow becomes:

1. The driver calls each detector and collects all diagnostics.
2. For each diagnostic, the driver consults
   `lintian_overrides::iter_overrides` and either keeps it, drops it, or
   marks it as overridden in the result.
3. The driver applies the actions of the surviving diagnostics.

This eliminates the per-fixer `NoChangesAfterOverrides` bookkeeping and
guarantees overrides are applied consistently — a common audit complaint
about today's code.

## Multiple actions per diagnostic

`Diagnostic.actions` is a `Vec`, not a single action, so a detector can
report:

* The minimal fix and an optional follow-up (e.g. set the field *and* remove a
  redundant override file).
* Alternative fixes, exposed via an enum tag on each action describing which
  group it belongs to. The simplest representation is an outer
  `Vec<ActionPlan>`, where each plan is a self-consistent set of actions:

  ```rust
  pub struct Diagnostic {
      pub issue:   LintianIssue,
      pub message: String,
      pub plans:   Vec<ActionPlan>,  // alternatives; first is the default
  }
  pub struct ActionPlan {
      pub label:   String,         // shown in LSP code-action menu
      pub actions: Vec<Action>,    // applied as a unit
  }
  ```

  The batch driver picks `plans[0]`. The LSP surfaces all of them as code
  actions.

## Action ordering and conflicts

When several detectors run on the same tree, two diagnostics can produce
actions that touch the same byte range or the same deb822 field. The driver
needs a story for that:

* **Same-file edits.** Group actions by file and apply them through a single
  editor (`TemplatedControlEditor`, `ChangelogEditor`, …). Most actions are
  structural (set field X to Y), so two edits to the same field are
  detectable: the driver reports a conflict and skips the lower-certainty
  diagnostic.
* **Detector dependencies.** Today's `after` / `before` ordering between
  fixers (`builtin_fixers.rs`) maps cleanly onto detector dependencies — it
  controls the order in which detectors *observe* the tree. Action appliers
  are commutative within a file when the actions touch disjoint paragraphs/
  ranges; the driver only needs to re-run a detector if an applied action
  invalidates its previous observation. In practice this means detectors run
  in topological order, actions are applied between passes, and the next pass
  sees the updated tree.
* **Re-detection after apply.** For most fixers there is no need: a detector
  that emits "set Priority on binary foo" doesn't care that another detector
  rewrote `Vcs-Git`. For the few cases where one fix unblocks another (e.g.
  `out-of-date-standards-version` after `wrong-debian-qa-group-name`) the
  driver re-runs detectors that declare an `after` dependency on a detector
  whose actions actually fired.

## The escape hatch

Some fixers do things no plausibly-finite enum will cover — e.g.
`debian_watch_use_templates.rs` rewrites a watch file using a template engine,
and `homepage_field_uses_insecure_uri.rs` may make HTTP requests to verify a
URL is safe to switch.

Because actions are the LSP wire format and must be `serde`-serialisable
(see *Decisions* below), we cannot stash a trait object inside an action and
defer the work to apply time. The pattern is therefore: **move the work into
the detector**. For the URL-equivalence check, the detector does the network
call; if it succeeds, it emits a plain `Deb822Action::SetField`. If it
fails, it emits no diagnostic. For the watch-file rewriter and similar cases where the desired change cannot
be expressed as a finite set of structured actions, the escape hatch is to emit
a dedicated `Command` action (e.g. `Command::StripTrailingWhitespace { file }`)
rather than a `FilesystemAction::Write` that simply reserialises the whole
file. A `Command` is still a plain, serialisable enum variant — it carries
enough parameters for the applier to execute it deterministically — but it
delegates the actual byte-level transformation to a purpose-built applier
rather than baking the result into the wire payload at detection time.

This means detection can be more expensive than today's per-fixer detection,
and that detectors may need network access. That's fine: we already gate
network-using fixers on `FixerPreferences::net_access`, and detection is
still strictly read-only with respect to the working tree.

## What stays the same

* The `LintianIssue` type, override file format, and `FixerPreferences` are
  unchanged.
* `BuiltinFixerRegistration` keeps its name, dependency lists, and
  `inventory::collect!` registration — only the body changes from "do
  everything" to "return diagnostics".
* The CLI behaviour of `lintian-brush` is unchanged: the driver just gains an
  apply step between detection and commit.
* Existing tests continue to work: a test that asserts "after running fixer X,
  debian/control looks like Y" still passes because the detector + applier
  combination produces the same on-disk result.

## Migration

There are ~150 fixers. Migrating them all in one change would be unreviewable.
A workable order:

1. Land the `Detector`, `Diagnostic`, `Action`, and applier types alongside
   the existing `BuiltinFixer` trait. Both registries coexist.
2. Implement the driver in two modes: "legacy fixer" (calls
   `BuiltinFixer::apply`) and "detector + applier" (calls `Detector::detect`,
   filters by overrides, applies actions). The legacy mode is the default.
3. Port one fixer at a time. Start with the deb822-only ones
   (`no_priority_field`, `homepage_field_uses_insecure_uri`,
   `vcs_field_*`, …) since they map cleanly onto the deb822 action variants.
4. As fixers move over, central override handling kicks in for them and the
   per-fixer `should_fix` calls disappear.
5. Once everything is ported, retire `BuiltinFixer`, `BuiltinFixerWrapper`,
   and the `declare_fixer!` macro. Replace with `declare_detector!`.

Each step is independently shippable and testable, and the legacy mode lets
us defer porting awkward fixers (e.g. those that shell out, or the watch-file
template rewriter) until last.

## Decisions

* **Scope of `Action`.** Per-file-type enums (`Deb822Action`,
  `MakefileAction`, `ChangelogAction`, `WatchAction`, `FilesystemAction`)
  wrapped in an outer `Action` enum dispatched on file kind. Each per-file
  applier matches only its own variants, and a new file family can be added
  without touching unrelated appliers.
* **Wire format.** `Action` and the per-file enums all derive
  `serde::Serialize` / `serde::Deserialize` (with `#[serde(tag = "kind")]`
  / `#[serde(tag = "op")]` so the JSON is self-describing) and are the LSP
  wire format directly. This rules out `Box<dyn AppliedAction>` on the
  boundary, and it means heavy work (network probes, template rendering) must
  happen in the detector rather than at apply time. Where the desired change
  cannot be expressed as a structured `Deb822Action` or similar, detectors
  emit a `Command` variant (e.g. `Command::StripTrailingWhitespace { file }`)
  whose applier performs the transformation deterministically. There is no
  `Action::Custom`.

