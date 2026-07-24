# RFC: Lazy, fact-driven detectors

## Summary

Detectors today are blocking procedures that perform expensive I/O (mostly
remote lookups) while holding the workspace lock. This serialises detection,
DoSes upstreams with duplicate requests, and stalls the editor.

This RFC redefines detectors as **pure, non-blocking functions of
`(workspace, facts)`**. A detector never does I/O. When it needs data it does
not have, it emits the diagnostics it *can* produce now and returns a set of
**fact wants** describing what would let it do better. All I/O is owned by a
single `FactStore` that guarantees each fact is fetched at most once. An
**engine** (the `lintian-brush` processor, or the `debian-lsp` server) drives
detectors to a fixpoint, fetching wants between rounds.

The detector contract is independent of how facts are stored. `debian-lsp`
should back the store with something persistent to inherit invalidation and
cancellation; `lintian-brush` uses a plain store and an explicit fixpoint loop.
Both share one detector trait and one fixpoint algorithm.

## Motivation

The current shape causes four concrete problems:

1. **Duplicate remote queries.** Multiple detectors independently fetch the
   same datum (e.g. "is package X in the archive?"), so a single edit can fan
   out into many identical requests - possibly enough to look like an attack to
   the upstream.
2. **Editor stalls.** Detection holds the workspace lock across network
   round-trips, so the LSP server cannot apply the user's next keystroke until
   the network returns.
3. **No parallelism.** The lock serialises detectors that have no real data
   dependency on each other. This significantly slows down both the LSP and
   lintian-brush.
4. **Eager action computation.** LSP does not require all code actions up
   front; it asks for actions per-diagnostic via `codeAction` and resolves the
   chosen one via `codeAction/resolve`. Computing every action eagerly is
   wasted work.

The unifying fix is to separate *deciding what data is needed* (cheap, pure,
parallel) from *acquiring it* (the only place I/O lives, deduplicated and
cancellable).

## Guide-level explanation

There are three roles, with strictly separated capabilities:

- **Detectors** are pure. They read the workspace and a read-only `FactView`,
  and return diagnostics plus wants. They cannot fetch, cannot block, cannot
  mutate. This is enforced by the types: a detector is handed `&dyn FactView`,
  which has no fetch or write method.
- **Fact Producers** know how to acquire one kind of fact. They are `async` and may
  await *other* facts (fact A's producer can request fact B). They are the only
  code that touches the network.
- **The engine** owns the `FactStore`. It runs detectors, collects wants,
  dispatches producers (single-flight), and re-runs detectors whose wants have
  landed, until no new wants appear.

The key invariant that makes this terminate and stay coherent: **a `FactKey`
is a complete content-address of a question, including every local input the
answer depends on.** "Does `libfoo-dev` exist in unstable?" and "does
`libbar-dev` exist in unstable?" are different keys; if the user edits the
dependency name, the old key is simply no longer asked. Facts are therefore
**immutable per key** and **monotonic** (once known, they stay known; learning
a fact never retracts a diagnostic, only refines or adds). Local file content
is *not* a fact - it is read directly from the workspace, which is cheap and,
under salsa, already change-tracked.

## Reference-level explanation

### Facts

```rust
/// Complete content-address of a question. Two keys are equal iff they ask
/// exactly the same question, including all local inputs the answer depends on.
/// `Eq + Hash + Clone`, cheap to clone (intern or `Arc` the payload).
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct FactKey(/* opaque */);

#[derive(Clone)]
pub struct Fact {
    pub value: FactValue,
    pub provenance: Provenance,
    /// When this fact was acquired. Metadata only - see "Refresh".
    pub acquired_at: SystemTime,
}

#[derive(Clone)]
pub enum Provenance {
    Local,                 // derived without I/O
    Remote { source: Arc<str> },
}
```

`query` is **infallible**: a failed fetch is a state, not an error a detector
must propagate.

```rust
pub enum FactQuery {
    /// We have it.
    Known(Arc<Fact>),
    /// A fetch is in flight; ask again later.
    Pending,
    /// A producer is registered but not started; the engine should schedule it.
    Fetchable,
    /// No producer exists for this key. Degrade gracefully; never retry.
    Unfetchable,
    /// We tried and failed. `retry_after` is `None` if not worth retrying.
    Failed { error: Arc<FetchError>, retry_after: Option<SystemTime> },
}
```

The capability split:

```rust
/// Read-only view handed to detectors. No fetch, no write, no block.
pub trait FactView: Send + Sync {
    fn query(&self, key: &FactKey) -> FactQuery;
}

/// Read-write store owned by the engine.
pub trait FactStore: FactView {
    fn set(&self, key: FactKey, fact: Fact);

    /// Register how to acquire a fact. Idempotent; last registration wins.
    fn register_producer(&self, key: FactKey, producer: Producer);

    /// Acquire a fact, awaiting if a fetch is already in flight. **Single-flight:
    /// at most one fetch per key is ever live; concurrent callers share it.**
    fn fetch(&self, key: &FactKey) -> BoxFuture<'static, Result<Arc<Fact>, FetchError>>;
}
```

> **Single-flight is a guarantee, not an emergent property.** This is the whole
> point of the redesign (problem 1). It must be enforced inside the store and
> covered by a test asserting fetch count == 1 under concurrent demand - not
> left to whether the scheduler happens to coalesce.

```rust
pub type Producer = Arc<
    dyn Fn(FetchCtx) -> BoxFuture<'static, Result<Fact, FetchError>> + Send + Sync,
>;

impl FetchCtx {
    /// Recursively acquire a dependency, also single-flight.
    pub async fn fetch(&self, key: &FactKey) -> Result<Arc<Fact>, FetchError>;
    /// Honoured cooperatively; the engine cancels stale work on document change.
    pub fn cancellation(&self) -> &CancellationToken;
}
```

### Diagnostics, wants and resolutions

A single channel for "what data would help," tagged with *why*, so the engine
can apply policy uniformly:

```rust
pub struct FactWant {
    pub key: FactKey,
    pub reason: WantReason,
    pub priority: Priority, // Interactive | Background
}

pub enum WantReason {
    /// Might surface a diagnostic that does not exist yet.
    ToDetect,
    /// Would refine an existing diagnostic (sharper range/message/severity).
    Refine(DiagId),
    /// Would unlock additional actions for an existing diagnostic.
    MoreActions(DiagId),
}
```

```rust
/// Stable *logical* identity of a diagnostic, e.g.
/// hash(detector_name, normalized_location, rule_code). NOT a per-run UUID.
/// Re-running a detector with more facts should yield the SAME id so a refined
/// diagnostic replaces the coarse one instead of duplicating it.
///
/// This is a best-effort identity, not a correctness invariant. If a detector
/// computes a slightly different id across rounds (e.g. the location moved
/// because the message got sharper), the coarse diagnostic simply isn't
/// replaced and both surface - a harmless duplicate, not a wrong answer. We
/// tolerate those false positives rather than demand a perfectly stable id.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct DiagId(/* opaque */);

pub struct Diagnostic {
    pub id: DiagId,
    pub range: Range,
    pub severity: Severity,
    pub message: String,
    pub resolutions: Vec<Resolution>,
}

pub enum Resolution {
    /// Fully computed; apply directly.
    Ready { id: String, plan: ActionPlan },
    /// Needs more facts. Surfaced to LSP as an unresolved code action;
    /// `payload` round-trips through codeAction/resolve.
    Deferred {
        id: String,
        payload: serde_json::Value,
        /// Document version `payload` was computed against; resolve must
        /// re-check or no-op if the document has moved on.
        version: DocVersion,
        wants: Vec<FactKey>,
    },
}
```

### The detector trait

The required method stays `detect` - the synchronous, fact-free pass the
existing fixers already implement. `detect_step` is the new fact-aware pass,
provided with a default that simply calls `detect` and reports no wants. A
detector opts into facts by *overriding* `detect_step`; until it does, it
behaves exactly as today.

```rust
pub struct DetectOutput {
    pub diagnostics: Vec<Diagnostic>,
    pub wants: Vec<FactWant>,
}

pub trait Detector: Send + Sync {
    fn name(&self) -> &str;

    /// The fact-free pass: detect issues from the workspace alone. This is
    /// the method existing detectors implement; it does whatever blocking
    /// work it needs (and, for the not-yet-ported `Network`/`Subprocess`
    /// detectors, still blocks on I/O). The lazy LSP engine therefore only
    /// drives detectors that have *overridden* `detect_step`; one still
    /// relying on the blocking default is run by the batch engine, or on an
    /// explicit non-keystroke action, never on the hot path.
    fn detect(
        &self,
        ws: &dyn Workspace,
        prefs: &FixerPreferences,
    ) -> Result<Vec<Diagnostic>, FixerError>;

    /// One pure, non-blocking pass over the workspace and currently-known
    /// facts. Returns diagnostics computable *now* plus wants that would
    /// improve the result. An overriding implementation MUST NOT perform I/O
    /// or block.
    ///
    /// The default defers to `detect` and wants nothing - correct for every
    /// detector that does no I/O, and a safe (if blocking) bridge for those
    /// that still do until they are ported.
    fn detect_step(
        &self,
        ws: &dyn Workspace,
        _facts: &dyn FactView,
        prefs: &FixerPreferences,
    ) -> Result<DetectOutput, FixerError> {
        Ok(DetectOutput {
            diagnostics: self.detect(ws, prefs)?,
            wants: Vec::new(),
        })
    }
}
```

The detector body sees only `&dyn FactView` inside `detect_step`. The store -
and the fixpoint that drives `detect_step` to convergence - stays with the
engine (`run_to_fixpoint`, below), not on the trait.

### Migrating the existing detectors

The default `detect_step` means the migration is opt-in and mechanical:

- A detector that does no I/O needs **no change** - the default `detect_step`
  calls its existing `detect` and reports no wants, so a single fixpoint round
  reproduces today's behaviour exactly.
- A detector that does blocking I/O today (the `Network`/`Subprocess` cost
  classes - e.g. `upstream-metadata-file`) is the real migration: the I/O moves
  into a producer keyed by a `FactKey`, and an **overriding** `detect_step`
  emits a want for that key instead of blocking. `detect` can then either
  delegate to the engine or be left as the legacy blocking path. These convert
  one detector at a time.
- The `declare_detector!` macro keeps its current `detect:` surface, which
  populates `detect`. A new optional `detect_step:` clause lets a ported
  detector supply the fact-aware pass; omitting it inherits the default. The
  existing declarations compile unchanged.

### Termination and monotonicity

`run_to_fixpoint`:

```text
round = 0
loop:
    out = detector.detect_step(ws, facts, prefs)      // pure
    merge out.diagnostics  (by DiagId: refined replaces coarse)
    wants = out.wants filtered by FetchPolicy
    if wants is empty: break                          // fixpoint reached
    fetch all wants (single-flight)                   // the only I/O
    round += 1
    if round > MAX_ROUNDS: warn(detector, outstanding wants); break
```

This terminates **only because facts are monotonic**: each round either learns
\>=1 new fact or stops, and learning a fact never makes a detector un-want what
it already has. `MAX_ROUNDS` is **8** - a backstop against producers that chain
unboundedly or against a buggy detector that re-wants a satisfied key; hitting
it is a warning, not a correctness failure (you simply get a slightly
less-refined result). Real fact chains are short (A reveals you need B reveals
C); 8 leaves generous headroom while still catching a runaway loop. The
original "run twice then warn" rule is the `MAX_ROUNDS = 2` special case and
under-detects when facts chain; the general fixpoint handles chains.

### Fetch policy

The engine chooses which wants to act on by reason and context:

| Engine / mode | `ToDetect` | `Refine` | `MoreActions` |
|---|---|---|---|
| `lintian-brush` | always | always | only for diagnostics being fixed |
| `debian-lsp`, lazy | on demand | on demand | only on `codeAction` for that diag |
| `debian-lsp`, eager | background | background | background |

### `lintian-brush` engine

A `DetectorProcessor` runs detection and fixing in two phases.

**Detection (parallel, to fixpoint).**

```text
todo = all detectors
while todo not empty:
    run detect_step for each detector in `todo` IN PARALLEL (shared FactView)
    for each detector:
        if no wants -> retire it; collect diagnostics (dedup by DiagId)
        else        -> record (detector, wants); keep diagnostics it gave
    fetch the union of recorded wants (single-flight, parallel)
    todo = detectors all of whose recorded wants are now resolved
           (Known | Unfetchable | Failed-no-retry - i.e. terminal states)
```

Detectors run in parallel because `detect_step` is pure and shares an immutable
`FactView`; the only shared mutable state is the store, which is internally
synchronised and single-flighting. A detector blocked on a `Fetchable`/`Pending`
fact is simply not in `todo` until that fact reaches a terminal state, so no
detector ever blocks a thread on I/O.

**Fixing (serial).**

```text
for each diagnostic the policy says to fix:
    resolve actions (fetching MoreActions facts for this diagnostic only)
    choose an action plan
apply chosen plans SERIALLY to the workspace
after each applied plan:
    the workspace changed -> any FactKey that encoded the edited input is now
    a *different* key, so dependent detectors naturally re-derive against new
    keys. Re-queue detectors whose inputs the edit touched.
    exclude (DiagId, action id) pairs already seen, to converge.
```

Because edits change *inputs*, and inputs are part of the key, there is no
"stale fact" to invalidate after an edit - the post-edit detector asks a new
question. Old facts remain valid answers to old questions and are harmless.

### `debian-lsp` engine

**On document change.** Run `detect_step` once for every detector against
currently-cached facts and **publish diagnostics immediately**. Never wait on a
fact to publish. This satisfies problem 2: the lock is held only for the pure
pass.

**Eager prefetch.** As soon as diagnostics carry wants, schedule the producers
in the background (`Priority::Background`). When facts land, re-run the affected
detectors, re-publish with a short debounce. So by the time the user hovers or
requests actions, the data is usually already present.

**`codeAction`.** Return `Ready` resolutions inline. For `Deferred`
resolutions, return unresolved `CodeAction`s carrying the `payload`. Acting on
`MoreActions` wants here (not eagerly) implements problem 4.

**`codeAction/resolve`.** If the `MoreActions` facts are now `Known`, compute
and return the `WorkspaceEdit`. **Check `DocVersion`:** the user keeps typing
between `codeAction` and resolve, so if the document moved past `payload`'s
version, recompute against the current document or return no edit - never apply
a plan to a workspace that has shifted under it.

**Cancellation.** When the document changes, in-flight fetches for the old
version are waste. Under the salsa backing (below) this is automatic; with a
raw store, the engine cancels the superseded `FetchCtx` tokens.

### Testing

The payoff of purity: detector tests are deterministic and network-free.

```rust
// A FactView backed by a literal map; no producers, no I/O.
let facts = MapFactView::from([
    (key_pkg_in_archive("libfoo-dev"), Fact::known_remote(true)),
]);
let out = MyDetector.detect_step(&ws, &facts, &prefs)?;
assert_eq!(out.diagnostics.len(), 1);
assert!(out.wants.is_empty());
```

Required tests:

- **Single-flight:** N concurrent `fetch` of one key ⇒ producer invoked once.
- **Fixpoint convergence:** a detector whose producer chain is A→B→C reaches a
  stable diagnostic set without hitting `MAX_ROUNDS`.
- **Refine, don't duplicate:** coarse-then-refined runs over the same `DiagId`
  yield one diagnostic, the refined one.
- **Graceful degradation:** `Unfetchable`/`Failed` ⇒ detector emits the
  coarse diagnostic, no error propagates.
- **Resolve staleness:** `codeAction/resolve` against a stale `DocVersion`
  no-ops or recomputes.

## Producer I/O model

Producers are **`async` throughout**, on tokio. This matches the LSP side
(tower-lsp is already tokio) and lets a producer await sibling facts
(`FetchCtx::fetch`) without a thread-pool hop. `lintian-brush`, which is
otherwise a synchronous batch tool, owns a small multi-threaded tokio runtime
and drives the fixpoint with `Runtime::block_on` at the top of the detection
phase; detectors and fixers below that boundary stay synchronous.

Concretely:

- `FactStore::fetch` and `FetchCtx::fetch` return `BoxFuture`, as written above.
- A `Producer` is an `async` closure (`Fn(FetchCtx) -> BoxFuture<…>`). The only
  `.await` points inside a producer are network/subprocess calls and recursive
  `ctx.fetch` calls; producers never block the runtime thread (use
  `spawn_blocking` for unavoidable sync I/O such as `gpg`).
- The single-flight machinery lives in the store as an async primitive: the
  first caller for a key installs a shared `Shared<BoxFuture>` (or a
  `tokio::sync::OnceCell`/broadcast latch); concurrent callers await the same
  future. No producer is entered twice for one key.
- `run_to_fixpoint` is itself `async`. The batch driver is the single place
  that bridges async to the synchronous CLI: it `block_on`s `run_to_fixpoint`
  on the engine runtime. `Detector::detect`/`detect_step` stay synchronous -
  they are the pure per-round passes the async fixpoint calls between fetches,
  not async themselves.

## Settled decisions

- **`Failed` retry policy.** The store owns a default backoff; a producer may
  override `retry_after` on the `Failed` it returns. Backoff lives in one place
  by default, but a producer that knows better (e.g. an HTTP `Retry-After`
  header) can set its own.
- **`FactKey` interning.** No interning. Keys keep `Arc<…>` payloads and are
  hashed directly; the map can be a plain `DashMap` (or sharded
  `RwLock<HashMap>`). Revisit only if profiling shows key hashing on the hot
  path.

## Possible follow-ups

- **Persisting facts across `lintian-brush` runs.** A small on-disk cache (with
  provenance) could avoid re-querying upstreams between CI runs. Out of scope
  for now; facts live only for the duration of a run. If pursued later, the
  open piece is the eviction story, since facts are otherwise immutable.
