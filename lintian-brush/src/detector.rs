use crate::FixerError;
use debian_workspace::fs_workspace::FsWorkspace;
use debian_workspace::{Trigger, Workspace};

/// Rough indication of a detector's runtime cost.
///
/// Annotated on each detector via `cost:` in [`declare_detector!`]. The
/// lintian-brush CLI ignores this — it always runs every selected
/// detector. LSP hosts use it to schedule work: cheap detectors can run
/// on every keystroke, expensive ones only on idle/save/explicit
/// request.
///
/// The variants are ordered cheapest → most expensive; comparisons via
/// the derived `PartialOrd` reflect that ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DetectorCost {
    /// Pure parse and in-memory check. Safe to run on every keystroke.
    Cheap,
    /// Walks the working tree, reads files outside the immediate trigger
    /// (lintian data files, maintscripts, override globs). Local I/O
    /// only — no network, no subprocess. Fine on a debounced idle tick.
    Filesystem,
    /// Forks a subprocess (e.g. `git ls-remote`, `gpg`, `dpkg-parsechangelog`).
    /// Local but slow; avoid on every keystroke.
    Subprocess,
    /// Talks to the network. Should only run on explicit user action
    /// (save / "scan now") in an LSP context.
    Network,
}

/// A detector reads a Debian source package and emits
/// [`Diagnostic`](crate::diagnostic::Diagnostic)s describing what (if
/// anything) needs fixing, together with the [`Action`](crate::diagnostic::Action)s
/// that would fix it. Detectors do *not* mutate the tree.
///
/// Detectors carry no `basedir`/`package`/`current_version` arguments —
/// those are reachable through the workspace — so the same detector
/// works in the lintian-brush CLI (with a [`FsWorkspace`]) and in
/// an LSP host that has no on-disk basedir for the open buffer.
///
/// The lintian-brush CLI driver picks up every registered detector via
/// [`crate::builtin_fixers::get_builtin_fixers`] and applies it through
/// [`detect_and_fix`].
pub trait Detector: Send + Sync {
    /// Stable name of the detector. Matches the corresponding fixer name.
    fn name(&self) -> &'static str;

    /// Lintian tags this detector's diagnostics correspond to.
    fn lintian_tags(&self) -> &'static [&'static str];

    /// What workspace state this detector reads.
    ///
    /// LSP hosts use this to skip detectors whose inputs haven't changed.
    /// The default `&[]` means "no declared triggers" — the LSP host
    /// should treat that as "always run" (for the detectors that haven't
    /// been annotated yet) and the CLI ignores it either way.
    fn triggers(&self) -> &'static [Trigger] {
        &[]
    }

    /// Rough cost class. See [`DetectorCost`] for the meaning of each
    /// variant. Defaults to `Cheap`; expensive detectors should override.
    fn cost(&self) -> DetectorCost {
        DetectorCost::Cheap
    }

    /// Detect issues in `ws` and return one [`Diagnostic`] per issue.
    ///
    /// `Ok(vec![])` means "nothing to fix, no error". `Err(NoChanges)` is
    /// also legal (and meaningfully equivalent) — detectors that compute
    /// "nothing to do" lazily often find that shape easier.
    fn detect(
        &self,
        ws: &dyn Workspace,
        preferences: &crate::FixerPreferences,
    ) -> Result<Vec<crate::diagnostic::Diagnostic>, FixerError>;

    /// Optional: customise the description used in the resulting
    /// [`crate::FixerResult`]. Defaults to
    /// [`crate::builtin_fixers::default_describe`].
    ///
    /// Each entry in `fixed` pairs a diagnostic with the [`ActionPlan`]
    /// the applier picked for it, so the describer can use the picked
    /// plan's `label` directly without re-running the selection logic.
    fn describe(
        &self,
        fixed: &[(crate::diagnostic::Diagnostic, crate::diagnostic::ActionPlan)],
        actions: &[crate::diagnostic::Action],
    ) -> String {
        crate::builtin_fixers::default_describe(fixed, actions)
    }

    /// Detect issues in `workspace` and apply the resulting actions.
    ///
    /// Runs [`detect`](Self::detect) and feeds its diagnostics through
    /// [`crate::builtin_fixers::apply_diagnostics_with`], using
    /// [`describe`](Self::describe) for the resulting description.
    ///
    /// Returns [`FixerError::NoChanges`] if the detector emitted nothing,
    /// and [`FixerError::NoChangesAfterOverrides`] if every diagnostic was
    /// filtered out by lintian overrides.
    fn apply(
        &self,
        workspace: &FsWorkspace,
        preferences: &crate::FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let diagnostics = self.detect(workspace, preferences)?;
        crate::builtin_fixers::apply_diagnostics_with(
            workspace.base_path(),
            &diagnostics,
            preferences,
            &|fixed, actions| self.describe(fixed, actions),
        )
    }
}

/// Run `f`, converting any panic into [`FixerError::Panic`].
fn catch_panic<T>(f: impl FnOnce() -> Result<T, FixerError>) -> Result<T, FixerError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(panic_payload) => {
            let message = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic payload".to_string()
            };
            let backtrace = std::backtrace::Backtrace::capture();
            let backtrace = if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
                Some(backtrace)
            } else {
                None
            };
            Err(FixerError::Panic { message, backtrace })
        }
    }
}

/// Run a [`Detector`]'s detection phase against an on-disk package,
/// catching panics, and filter the diagnostics into a
/// [`DiagnosticPlan`](crate::builtin_fixers::DiagnosticPlan).
///
/// This performs no tree mutation: it runs [`Detector::detect`] followed
/// by [`crate::builtin_fixers::plan_diagnostics`]. A panicking detector
/// is reported as [`FixerError::Panic`] rather than unwinding. The
/// returned plan is applied separately via
/// [`crate::builtin_fixers::apply_plan`], so a caller can decide whether
/// there is anything worth doing before it mutates the working tree.
///
/// Detectors that need per-run configuration read it from
/// `preferences.extra_env`.
pub fn detect_and_plan(
    detector: &dyn Detector,
    workspace: &FsWorkspace,
    preferences: &crate::FixerPreferences,
) -> Result<crate::builtin_fixers::DiagnosticPlan, FixerError> {
    catch_panic(|| {
        let diagnostics = detector.detect(workspace, preferences)?;
        crate::builtin_fixers::plan_diagnostics(workspace.base_path(), &diagnostics, preferences)
    })
}

/// Run a [`Detector`] against an on-disk package, catching panics.
///
/// Wraps [`Detector::apply`] so that a panicking detector is reported as
/// [`FixerError::Panic`] rather than unwinding. This is the end-to-end
/// convenience; [`crate::run_lintian_fixer`] instead uses
/// [`detect_and_plan`] so it can split detection from tree mutation.
pub fn detect_and_fix(
    detector: &dyn Detector,
    workspace: &FsWorkspace,
    preferences: &crate::FixerPreferences,
) -> Result<crate::FixerResult, FixerError> {
    catch_panic(|| detector.apply(workspace, preferences))
}

/// Inventory entry for a [`Detector`].
///
/// Submitted automatically by [`declare_detector!`]; iterated via
/// [`iter_detectors`].
pub struct DetectorRegistration {
    /// Stable name of the detector.
    pub name: &'static str,
    /// Lintian tags this detector addresses.
    pub lintian_tags: &'static [&'static str],
    /// Constructor for an instance.
    pub create: fn() -> Box<dyn Detector>,
    /// Detectors that must run before this one.
    pub after: &'static [&'static str],
    /// Detectors that must run after this one.
    pub before: &'static [&'static str],
    /// What workspace state this detector reads. See [`Detector::triggers`].
    pub triggers: &'static [Trigger],
    /// Rough cost class. See [`DetectorCost`] and [`Detector::cost`].
    pub cost: DetectorCost,
}

inventory::collect!(DetectorRegistration);

/// Iterate every registered [`Detector`].
pub fn iter_detectors() -> impl Iterator<Item = Box<dyn Detector>> {
    inventory::iter::<DetectorRegistration>
        .into_iter()
        .map(|reg| (reg.create)())
}

/// Iterate every registered [`DetectorRegistration`] without
/// instantiating a [`Detector`].
///
/// Hosts that want to filter detectors by `cost`, `triggers`, or
/// `name` before deciding whether to run them (e.g. an LSP server
/// that runs a subset on every keystroke) should iterate the
/// registrations directly and only call [`DetectorRegistration::create`]
/// on the survivors. The CLI driver uses [`iter_detectors`] instead
/// because it always runs everything.
pub fn iter_detector_registrations() -> impl Iterator<Item = &'static DetectorRegistration> {
    inventory::iter::<DetectorRegistration>.into_iter()
}

/// Error indicating an unknown detector was requested.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownDetector(pub String);

impl std::fmt::Display for UnknownDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Unknown detector: {}", self.0)
    }
}

impl std::error::Error for UnknownDetector {}

/// Select detectors by name from a list, applying include/exclude sets.
///
/// `names` keeps only the listed detectors; `exclude` drops them. An
/// entry that appears in either set but matches no detector returns
/// [`UnknownDetector`].
pub fn select_detectors(
    detectors: Vec<Box<dyn Detector>>,
    names: Option<&[&str]>,
    exclude: Option<&[&str]>,
) -> Result<Vec<Box<dyn Detector>>, UnknownDetector> {
    use std::collections::HashSet;
    let mut select_set = names.map(|names| names.iter().cloned().collect::<HashSet<_>>());
    let mut exclude_set = exclude.map(|exclude| exclude.iter().cloned().collect::<HashSet<_>>());
    let mut ret = vec![];
    for d in detectors.into_iter() {
        if let Some(select_set) = select_set.as_mut() {
            if !select_set.remove(d.name()) {
                if let Some(exclude_set) = exclude_set.as_mut() {
                    exclude_set.remove(d.name());
                }
                continue;
            }
        }
        if let Some(exclude_set) = exclude_set.as_mut() {
            if exclude_set.remove(d.name()) {
                continue;
            }
        }
        ret.push(d);
    }
    if let Some(select_set) = select_set.filter(|x| !x.is_empty()) {
        Err(UnknownDetector(
            select_set.iter().next().unwrap().to_string(),
        ))
    } else if let Some(exclude_set) = exclude_set.filter(|x| !x.is_empty()) {
        Err(UnknownDetector(
            exclude_set.iter().next().unwrap().to_string(),
        ))
    } else {
        Ok(ret)
    }
}

/// Declare a [`Detector`] and register it.
///
/// Generates the `Detector` impl and an inventory submission that the CLI
/// driver picks up via [`crate::builtin_fixers::get_builtin_fixers`].
///
/// # Example
///
/// ```ignore
/// declare_detector! {
///     name: "homepage-field-uses-insecure-uri",
///     tags: ["homepage-field-uses-insecure-uri"],
///     detect: |ws, prefs| detect(ws, prefs),
/// }
/// ```
///
/// The `after`, `before` and `describe` clauses are optional. `describe`
/// takes `fn(&[Diagnostic], &[Action]) -> String`.
#[macro_export]
macro_rules! declare_detector {
    (
        name: $name:expr,
        tags: [$($tag:expr),* $(,)?],
        $(after: [$($after:expr),* $(,)?],)?
        $(before: [$($before:expr),* $(,)?],)?
        $(triggers: [$($trigger:expr),* $(,)?],)?
        $(cost: $cost:expr,)?
        detect: $detect_fn:expr
        $(, describe: $describe_fn:expr)?
        $(,)?
    ) => {
        struct DetectorImpl;

        impl $crate::detector::Detector for DetectorImpl {
            fn name(&self) -> &'static str { $name }
            fn lintian_tags(&self) -> &'static [&'static str] { &[$($tag),*] }

            fn triggers(&self) -> &'static [::debian_workspace::Trigger] {
                &[$($($trigger),*)?]
            }

            $(
            fn cost(&self) -> $crate::detector::DetectorCost {
                $cost
            }
            )?

            fn detect(
                &self,
                ws: &dyn ::debian_workspace::Workspace,
                preferences: &$crate::FixerPreferences,
            ) -> Result<Vec<$crate::diagnostic::Diagnostic>, $crate::FixerError> {
                let detect_fn: fn(
                    &dyn ::debian_workspace::Workspace,
                    &$crate::FixerPreferences,
                ) -> Result<Vec<$crate::diagnostic::Diagnostic>, $crate::FixerError> = $detect_fn;
                detect_fn(ws, preferences)
            }

            $(
            fn describe(
                &self,
                fixed: &[(
                    $crate::diagnostic::Diagnostic,
                    $crate::diagnostic::ActionPlan,
                )],
                actions: &[$crate::diagnostic::Action],
            ) -> String {
                let describe_fn: fn(
                    &[(
                        $crate::diagnostic::Diagnostic,
                        $crate::diagnostic::ActionPlan,
                    )],
                    &[$crate::diagnostic::Action],
                ) -> String = $describe_fn;
                describe_fn(fixed, actions)
            }
            )?
        }

        // The cost expression evaluates to either the user-supplied
        // `$cost` or — when the clause is omitted — `DetectorCost::Cheap`.
        const __COST: $crate::detector::DetectorCost = {
            #[allow(unused_mut, unused_assignments)]
            let mut c = $crate::detector::DetectorCost::Cheap;
            $(c = $cost;)?
            c
        };

        $crate::inventory::submit! {
            $crate::detector::DetectorRegistration {
                name: $name,
                lintian_tags: &[$($tag),*],
                create: || Box::new(DetectorImpl),
                after: &[$($($after),*)?],
                before: &[$($($before),*)?],
                triggers: &[$($($trigger),*)?],
                cost: __COST,
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock detector for select_detectors tests; doesn't actually detect
    /// anything.
    struct DummyDetector {
        name: &'static str,
        tags: &'static [&'static str],
    }

    impl Detector for DummyDetector {
        fn name(&self) -> &'static str {
            self.name
        }
        fn lintian_tags(&self) -> &'static [&'static str] {
            self.tags
        }
        fn detect(
            &self,
            _ws: &dyn Workspace,
            _preferences: &crate::FixerPreferences,
        ) -> Result<Vec<crate::diagnostic::Diagnostic>, FixerError> {
            unimplemented!()
        }
    }

    fn dummies() -> Vec<Box<dyn Detector>> {
        vec![
            Box::new(DummyDetector {
                name: "dummy1",
                tags: &["some-tag"],
            }),
            Box::new(DummyDetector {
                name: "dummy2",
                tags: &["other-tag"],
            }),
        ]
    }

    #[test]
    fn select_detectors_includes() {
        let result = select_detectors(dummies(), Some(["dummy1"].as_slice()), None).map(|m| {
            m.into_iter()
                .map(|d| d.name().to_string())
                .collect::<Vec<_>>()
        });
        assert_eq!(result, Ok(vec!["dummy1".to_string()]));
    }

    #[test]
    fn select_detectors_unknown_include() {
        assert!(select_detectors(dummies(), Some(["other"].as_slice()), None).is_err());
    }

    #[test]
    fn select_detectors_unknown_exclude() {
        assert!(select_detectors(
            dummies(),
            Some(["dummy"].as_slice()),
            Some(["some-other"].as_slice())
        )
        .is_err());
    }

    #[test]
    fn select_detectors_excludes() {
        let result = select_detectors(
            dummies(),
            Some(["dummy1"].as_slice()),
            Some(["dummy2"].as_slice()),
        )
        .map(|m| {
            m.into_iter()
                .map(|d| d.name().to_string())
                .collect::<Vec<_>>()
        });
        assert_eq!(result, Ok(vec!["dummy1".to_string()]));
    }

    #[test]
    fn triggers_reach_registered_detector() {
        // The annotated `empty-debian-patches-series` detector declares a
        // single File trigger; this also verifies the macro plumbing.
        let det = inventory::iter::<DetectorRegistration>
            .into_iter()
            .find(|reg| reg.name == "empty-debian-patches-series")
            .expect("empty-debian-patches-series registered");
        let triggers = det.triggers;
        assert_eq!(triggers.len(), 1);
        assert!(matches!(
            triggers[0],
            Trigger::File("debian/patches/series")
        ));

        // Detectors without an explicit `triggers:` clause expose the
        // empty list (the trait default).
        let untriggered = DummyDetector {
            name: "untriggered",
            tags: &[],
        };
        assert!(untriggered.triggers().is_empty());
    }

    #[test]
    fn cost_reaches_registered_detector() {
        // `debian-watch-file-is-missing` opts in to the Network cost class.
        let net = inventory::iter::<DetectorRegistration>
            .into_iter()
            .find(|reg| reg.name == "debian-watch-file-is-missing")
            .expect("debian-watch-file-is-missing registered");
        assert_eq!(net.cost, DetectorCost::Network);
        assert_eq!((net.create)().cost(), DetectorCost::Network);

        // A detector that omits `cost:` falls back to Cheap.
        let cheap = inventory::iter::<DetectorRegistration>
            .into_iter()
            .find(|reg| reg.name == "empty-debian-patches-series")
            .expect("empty-debian-patches-series registered");
        assert_eq!(cheap.cost, DetectorCost::Cheap);
        assert_eq!((cheap.create)().cost(), DetectorCost::Cheap);
    }

    #[test]
    fn detector_cost_ordering_is_cheap_to_expensive() {
        // LSP hosts rely on the `PartialOrd` derivation reflecting cost.
        assert!(DetectorCost::Cheap < DetectorCost::Filesystem);
        assert!(DetectorCost::Filesystem < DetectorCost::Subprocess);
        assert!(DetectorCost::Subprocess < DetectorCost::Network);
    }
}
