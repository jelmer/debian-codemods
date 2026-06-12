//! Driver glue for running [`Detector`](crate::detector::Detector)s.
//!
//! This module owns:
//!
//! * [`apply_diagnostics`] / [`apply_diagnostics_with`] — the shared
//!   pipeline that filters diagnostics by lintian overrides and
//!   `preferences.minimum_certainty`, then drives
//!   [`crate::appliers::apply_actions`].
//! * [`default_describe`] — the default commit message generator.
//! * [`get_builtin_fixers`] — collects every registered
//!   [`Detector`](crate::detector::Detector) and sorts the result by
//!   `after`/`before` declarations.

use super::*;

/// Default describer: deduplicates the imperative labels of the
/// applied plans and joins them with newlines.
pub fn default_describe(
    fixed: &[(crate::diagnostic::Diagnostic, crate::diagnostic::ActionPlan)],
    _actions: &[crate::diagnostic::Action],
) -> String {
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<&str> = fixed
        .iter()
        .map(|(_, plan)| plan.label.as_str())
        .filter(|m| seen.insert(*m))
        .collect();
    if unique.len() == 1 {
        unique[0].to_string()
    } else {
        unique.join("\n")
    }
}

/// Default driver for fixers that emit [`Diagnostic`](crate::diagnostic::Diagnostic)s.
///
/// Filters diagnostics by lintian overrides and `preferences.minimum_certainty`,
/// then applies the first plan whose `opinionated` flag is satisfied by
/// `preferences.opinionated`. Opinionated plans only fire when the user
/// has opted in. The description is built via [`default_describe`].
pub fn apply_diagnostics(
    basedir: &std::path::Path,
    diagnostics: &[crate::diagnostic::Diagnostic],
    preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    apply_diagnostics_with(basedir, diagnostics, preferences, &default_describe)
}

/// The outcome of filtering a detector's diagnostics, before any tree
/// mutation has happened.
///
/// Produced by [`plan_diagnostics`] and consumed by [`apply_plan`].
/// Splitting the pipeline this way lets a caller decide whether there is
/// anything worth doing (the plan carries at least one action) before it
/// commits to mutating the working tree.
pub struct DiagnosticPlan {
    /// Diagnostics that survived filtering, each paired with the
    /// [`ActionPlan`](crate::diagnostic::ActionPlan) chosen for it.
    pub fixed: Vec<(crate::diagnostic::Diagnostic, crate::diagnostic::ActionPlan)>,
    /// Issues that were suppressed by a lintian override.
    pub overridden_issues: Vec<LintianIssue>,
    /// The flat list of actions from every chosen plan, in order.
    pub all_actions: Vec<crate::diagnostic::Action>,
    /// The lowest certainty across the fired diagnostics, where each
    /// diagnostic's certainty is its own confidence capped by the chosen
    /// plan's. `None` if neither side of any fired diagnostic declared one.
    pub min_actual_certainty: Option<Certainty>,
}

/// Filter a detector's diagnostics into a [`DiagnosticPlan`].
///
/// Drops diagnostics suppressed by lintian overrides, picks the first
/// [`ActionPlan`](crate::diagnostic::ActionPlan) whose `opinionated` flag
/// is satisfied by `preferences.opinionated`, then drops the diagnostic
/// if the diagnostic's certainty capped by that plan's falls below
/// `preferences.minimum_certainty`.
///
/// This phase performs no tree mutation. It returns
/// [`FixerError::NoChanges`] / [`FixerError::NoChangesAfterOverrides`] /
/// [`FixerError::NotCertainEnough`] when nothing actionable survives,
/// matching the errors [`apply_diagnostics_with`] used to return.
pub fn plan_diagnostics(
    basedir: &std::path::Path,
    diagnostics: &[crate::diagnostic::Diagnostic],
    preferences: &FixerPreferences,
) -> Result<DiagnosticPlan, FixerError> {
    use debian_analyzer::certainty_sufficient;

    if diagnostics.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let min_certainty = preferences.minimum_certainty;

    let mut fixed: Vec<(crate::diagnostic::Diagnostic, crate::diagnostic::ActionPlan)> = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut not_certain_enough: Vec<LintianIssue> = Vec::new();
    let mut min_actual_certainty: Option<Certainty> = None;
    let mut all_actions: Vec<crate::diagnostic::Action> = Vec::new();

    for diag in diagnostics {
        if let Some(issue) = &diag.issue {
            if !issue.should_fix(basedir) {
                overridden_issues.push(issue.clone());
                continue;
            }
        }
        let allow_opinionated = preferences.opinionated.unwrap_or(false);
        let Some(plan) = diag
            .plans
            .iter()
            .find(|p| !p.opinionated || allow_opinionated)
        else {
            continue;
        };
        // The change is only as certain as its weakest link: the
        // diagnostic's confidence that the issue is real, capped by the
        // chosen plan's confidence that it fixes the issue correctly.
        // `declared` stays None when neither side made a claim, so the
        // result reports no certainty rather than a synthetic `certain`.
        let declared = match (diag.certainty, plan.certainty) {
            (None, None) => None,
            (a, b) => Some(
                a.unwrap_or(Certainty::Certain)
                    .min(b.unwrap_or(Certainty::Certain)),
            ),
        };
        let actual_certainty = declared.unwrap_or(Certainty::Certain);
        if !certainty_sufficient(actual_certainty, min_certainty) {
            if let Some(issue) = &diag.issue {
                not_certain_enough.push(issue.clone());
            }
            continue;
        }
        all_actions.extend(plan.actions.iter().cloned());
        fixed.push((diag.clone(), plan.clone()));
        min_actual_certainty = match (min_actual_certainty, declared) {
            (None, c) => c,
            (Some(prev), None) => Some(prev),
            (Some(prev), Some(c)) => Some(prev.min(c)),
        };
    }

    if all_actions.is_empty() {
        if !overridden_issues.is_empty() && fixed.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        if !not_certain_enough.is_empty() && fixed.is_empty() {
            return Err(FixerError::NotCertainEnough(
                Certainty::Possible,
                min_certainty,
                not_certain_enough,
            ));
        }
        return Err(FixerError::NoChanges);
    }

    Ok(DiagnosticPlan {
        fixed,
        overridden_issues,
        all_actions,
        min_actual_certainty,
    })
}

/// Apply a [`DiagnosticPlan`], mutating the tree under `basedir`.
///
/// Runs every action in `plan.all_actions` and builds the resulting
/// [`FixerResult`], using `describe` for its description. Returns
/// [`FixerError::NoChanges`] if applying the actions produced no
/// observable change.
pub fn apply_plan(
    basedir: &std::path::Path,
    plan: DiagnosticPlan,
    describe: &dyn Fn(
        &[(crate::diagnostic::Diagnostic, crate::diagnostic::ActionPlan)],
        &[crate::diagnostic::Action],
    ) -> String,
) -> Result<FixerResult, FixerError> {
    let DiagnosticPlan {
        fixed,
        overridden_issues,
        all_actions,
        min_actual_certainty,
    } = plan;

    let changed = debian_workspace::appliers::apply_actions(basedir, &all_actions)?;
    if changed.is_empty() {
        // Detector said there was something to fix but applying produced no
        // observable change. Treat as NoChanges to avoid an empty commit.
        return Err(FixerError::NoChanges);
    }

    let description = describe(&fixed, &all_actions);
    let patch_name = fixed.iter().find_map(|(d, _)| d.patch_name.clone());
    let fixed_issues: Vec<LintianIssue> = fixed.into_iter().filter_map(|(d, _)| d.issue).collect();

    let mut builder = FixerResult::builder(description).fixed_issues(fixed_issues);
    if let Some(cert) = min_actual_certainty {
        builder = builder.certainty(cert);
    }
    if !overridden_issues.is_empty() {
        builder = builder.overridden_issues(overridden_issues);
    }
    if let Some(name) = patch_name {
        builder = builder.patch_name(name);
    }
    Ok(builder.build())
}

/// Like [`apply_diagnostics`], but lets the caller provide a custom
/// describer. The describer receives the diagnostics that actually fired
/// (after override / certainty filtering) and the flat list of actions
/// that were applied, and must return the description string used in the
/// resulting [`FixerResult`].
pub fn apply_diagnostics_with(
    basedir: &std::path::Path,
    diagnostics: &[crate::diagnostic::Diagnostic],
    preferences: &FixerPreferences,
    describe: &dyn Fn(
        &[(crate::diagnostic::Diagnostic, crate::diagnostic::ActionPlan)],
        &[crate::diagnostic::Action],
    ) -> String,
) -> Result<FixerResult, FixerError> {
    let plan = plan_diagnostics(basedir, diagnostics, preferences)?;
    apply_plan(basedir, plan, describe)
}

/// Topologically sort detector registrations based on their `after` /
/// `before` declarations.
///
/// Resolves both kinds of constraint into a single dependency graph and
/// performs Kahn's-algorithm sort with deterministic tie-breaking.
///
/// Ordering constraints (`after` / `before`) that reference a detector which
/// is not in `registrations` are ignored. This happens when a fixer is
/// excluded from the build by a feature flag: a constraint relative to a
/// fixer that never runs is vacuously satisfied.
///
/// # Panics
///
/// Panics if a circular dependency is detected.
fn topologically_sort_detectors<'a>(
    registrations: Vec<&'a crate::detector::DetectorRegistration>,
) -> Vec<&'a crate::detector::DetectorRegistration> {
    use std::collections::{HashMap, HashSet, VecDeque};

    let name_to_reg: HashMap<&str, &'a crate::detector::DetectorRegistration> =
        registrations.iter().map(|reg| (reg.name, *reg)).collect();

    // edge A -> B means "A must run before B".
    let mut adj_list: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();

    for reg in &registrations {
        adj_list.entry(reg.name).or_default();
        in_degree.entry(reg.name).or_insert(0);
    }

    for reg in &registrations {
        for dep in reg.after {
            if !name_to_reg.contains_key(dep) {
                continue;
            }
            adj_list.entry(*dep).or_default().push(reg.name);
            *in_degree.entry(reg.name).or_insert(0) += 1;
        }
    }

    for reg in &registrations {
        for dep in reg.before {
            if !name_to_reg.contains_key(dep) {
                continue;
            }
            adj_list.entry(reg.name).or_default().push(*dep);
            *in_degree.entry(*dep).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, &degree)| degree == 0)
        .map(|(&name, _)| name)
        .collect();

    let mut queue_vec: Vec<_> = queue.drain(..).collect();
    queue_vec.sort();
    queue.extend(queue_vec);

    let mut sorted = Vec::new();
    let mut processed = HashSet::new();

    while let Some(node) = queue.pop_front() {
        sorted.push(node);
        processed.insert(node);

        let mut neighbors = adj_list.get(node).cloned().unwrap_or_default();
        neighbors.sort();

        for neighbor in neighbors {
            if let Some(degree) = in_degree.get_mut(neighbor) {
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(neighbor);
                }
            }
        }

        let mut queue_vec: Vec<_> = queue.drain(..).collect();
        queue_vec.sort();
        queue.extend(queue_vec);
    }

    if sorted.len() != registrations.len() {
        let remaining: Vec<_> = registrations
            .iter()
            .filter(|reg| !processed.contains(reg.name))
            .map(|reg| reg.name)
            .collect();

        let mut cycle_msg = String::from("Circular dependency detected among fixers: ");
        cycle_msg.push_str(&remaining.join(", "));
        cycle_msg.push_str("\nDependency relationships:");

        for name in &remaining {
            if let Some(reg) = name_to_reg.get(name) {
                if !reg.after.is_empty() {
                    cycle_msg.push_str(&format!(
                        "\n  '{}' after: [{}]",
                        name,
                        reg.after.join(", ")
                    ));
                }
                if !reg.before.is_empty() {
                    cycle_msg.push_str(&format!(
                        "\n  '{}' before: [{}]",
                        name,
                        reg.before.join(", ")
                    ));
                }
            }
        }

        panic!("{}", cycle_msg);
    }

    sorted.iter().map(|name| name_to_reg[name]).collect()
}

/// Get all registered builtin detectors.
///
/// Iterates every [`Detector`](crate::detector::Detector) registered via
/// [`declare_detector!`](crate::declare_detector) and sorts the result by
/// `after` / `before` declarations.
pub fn get_builtin_fixers() -> Vec<Box<dyn crate::detector::Detector>> {
    let registrations: Vec<&'static crate::detector::DetectorRegistration> =
        inventory::iter::<crate::detector::DetectorRegistration>
            .into_iter()
            .collect();
    let sorted = topologically_sort_detectors(registrations);
    sorted.into_iter().map(|reg| (reg.create)()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_fixers_dependency_consistency() {
        // This test verifies that every registered detector has
        // consistent `after` / `before` declarations:
        // 1. No circular dependencies.
        // 2. All registered fixers are kept in the sorted output.
        // 3. Ordering constraints between two present fixers are respected.
        //
        // References to fixers that are absent from the current build (e.g.
        // excluded by a feature flag) are ignored, mirroring the sort itself.
        let registrations: Vec<&'static crate::detector::DetectorRegistration> =
            inventory::iter::<crate::detector::DetectorRegistration>
                .into_iter()
                .collect();
        let original_count = registrations.len();
        let sorted = topologically_sort_detectors(registrations.clone());

        assert_eq!(
            sorted.len(),
            original_count,
            "Topological sort lost some fixers! Expected {}, got {}",
            original_count,
            sorted.len()
        );

        let mut seen_names = std::collections::HashSet::new();
        for reg in &sorted {
            assert!(
                seen_names.insert(reg.name),
                "Duplicate fixer name in sorted output: {}",
                reg.name
            );
        }

        let name_to_index: std::collections::HashMap<_, _> = sorted
            .iter()
            .enumerate()
            .map(|(idx, reg)| (reg.name, idx))
            .collect();

        for (idx, reg) in sorted.iter().enumerate() {
            for dep in reg.after {
                let Some(dep_idx) = name_to_index.get(dep) else {
                    continue;
                };
                assert!(
                    dep_idx < &idx,
                    "Dependency ordering violated: '{}' (index {}) should run after '{}' (index {})",
                    reg.name, idx, dep, dep_idx
                );
            }
            for dep in reg.before {
                let Some(dep_idx) = name_to_index.get(dep) else {
                    continue;
                };
                assert!(
                    dep_idx > &idx,
                    "Dependency ordering violated: '{}' (index {}) should run before '{}' (index {})",
                    reg.name, idx, dep, dep_idx
                );
            }
        }
    }

    #[test]
    fn test_get_builtin_fixers() {
        let fixers = get_builtin_fixers();
        assert!(
            fixers.len() >= 2,
            "Expected at least 2 builtin fixers, found {}",
            fixers.len()
        );
        assert!(
            fixers
                .iter()
                .any(|f| f.name() == "control-file-with-CRLF-EOLs"),
            "CRLF fixer not found"
        );
        assert!(
            fixers.iter().any(|f| f.name() == "executable-desktop-file"),
            "executable-desktop-file fixer not found"
        );
    }

    use crate::detector::{detect_and_fix, Detector, DetectorRegistration};
    use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
    use debian_workspace::workspace::Workspace;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    /// Mock detector for the topological-sort tests.
    struct MockDetector {
        name: &'static str,
        tags: &'static [&'static str],
    }

    impl Detector for MockDetector {
        fn name(&self) -> &'static str {
            self.name
        }
        fn lintian_tags(&self) -> &'static [&'static str] {
            self.tags
        }
        fn detect(
            &self,
            _ws: &dyn Workspace,
            _preferences: &FixerPreferences,
        ) -> Result<Vec<Diagnostic>, FixerError> {
            Ok(Vec::new())
        }
    }

    fn detector_reg(
        name: &'static str,
        after: &'static [&'static str],
        before: &'static [&'static str],
    ) -> DetectorRegistration {
        fn make_a() -> Box<dyn Detector> {
            Box::new(MockDetector {
                name: "fixer-a",
                tags: &[],
            })
        }
        fn make_b() -> Box<dyn Detector> {
            Box::new(MockDetector {
                name: "fixer-b",
                tags: &[],
            })
        }
        fn make_c() -> Box<dyn Detector> {
            Box::new(MockDetector {
                name: "fixer-c",
                tags: &[],
            })
        }
        fn make_d() -> Box<dyn Detector> {
            Box::new(MockDetector {
                name: "fixer-d",
                tags: &[],
            })
        }
        let create: fn() -> Box<dyn Detector> = match name {
            "fixer-a" => make_a,
            "fixer-b" => make_b,
            "fixer-c" => make_c,
            "fixer-d" => make_d,
            _ => unreachable!(),
        };
        DetectorRegistration {
            name,
            lintian_tags: &[],
            create,
            after,
            before,
            triggers: &[],
            cost: crate::detector::DetectorCost::Cheap,
        }
    }

    #[test]
    fn test_topological_sort_no_dependencies() {
        let a = detector_reg("fixer-a", &[], &[]);
        let b = detector_reg("fixer-b", &[], &[]);
        let c = detector_reg("fixer-c", &[], &[]);
        let registrations = vec![&a, &b, &c];
        let sorted = topologically_sort_detectors(registrations);
        assert_eq!(sorted[0].name, "fixer-a");
        assert_eq!(sorted[1].name, "fixer-b");
        assert_eq!(sorted[2].name, "fixer-c");
    }

    #[test]
    fn test_topological_sort_simple_after() {
        let a = detector_reg("fixer-a", &[], &[]);
        let b = detector_reg("fixer-b", &["fixer-a"], &[]);
        let registrations = vec![&b, &a];
        let sorted = topologically_sort_detectors(registrations);
        assert_eq!(sorted[0].name, "fixer-a");
        assert_eq!(sorted[1].name, "fixer-b");
    }

    #[test]
    fn test_topological_sort_simple_before() {
        let a = detector_reg("fixer-a", &[], &["fixer-b"]);
        let b = detector_reg("fixer-b", &[], &[]);
        let registrations = vec![&b, &a];
        let sorted = topologically_sort_detectors(registrations);
        assert_eq!(sorted[0].name, "fixer-a");
        assert_eq!(sorted[1].name, "fixer-b");
    }

    #[test]
    fn test_topological_sort_chain() {
        let a = detector_reg("fixer-a", &[], &[]);
        let b = detector_reg("fixer-b", &["fixer-a"], &[]);
        let c = detector_reg("fixer-c", &["fixer-b"], &[]);
        let registrations = vec![&c, &a, &b];
        let sorted = topologically_sort_detectors(registrations);
        assert_eq!(sorted[0].name, "fixer-a");
        assert_eq!(sorted[1].name, "fixer-b");
        assert_eq!(sorted[2].name, "fixer-c");
    }

    #[test]
    fn test_topological_sort_complex_graph() {
        //     A
        //    / \
        //   B   C
        //    \ /
        //     D
        let a = detector_reg("fixer-a", &[], &["fixer-b", "fixer-c"]);
        let b = detector_reg("fixer-b", &["fixer-a"], &["fixer-d"]);
        let c = detector_reg("fixer-c", &["fixer-a"], &["fixer-d"]);
        let d = detector_reg("fixer-d", &["fixer-b", "fixer-c"], &[]);
        let registrations = vec![&d, &c, &b, &a];
        let sorted = topologically_sort_detectors(registrations);
        assert_eq!(sorted[0].name, "fixer-a");
        assert_eq!(sorted[3].name, "fixer-d");
        let middle: Vec<_> = sorted[1..3].iter().map(|r| r.name).collect();
        assert!(middle.contains(&"fixer-b"));
        assert!(middle.contains(&"fixer-c"));
    }

    #[test]
    #[should_panic(expected = "Circular dependency detected")]
    fn test_topological_sort_circular_dependency_simple() {
        let a = detector_reg("fixer-a", &["fixer-b"], &[]);
        let b = detector_reg("fixer-b", &["fixer-a"], &[]);
        topologically_sort_detectors(vec![&a, &b]);
    }

    #[test]
    #[should_panic(expected = "Circular dependency detected")]
    fn test_topological_sort_circular_dependency_complex() {
        // A -> B -> C -> A
        let a = detector_reg("fixer-a", &[], &["fixer-b"]);
        let b = detector_reg("fixer-b", &["fixer-a"], &["fixer-c"]);
        let c = detector_reg("fixer-c", &["fixer-b"], &["fixer-a"]);
        topologically_sort_detectors(vec![&a, &b, &c]);
    }

    #[test]
    fn test_topological_sort_ignores_missing_after_dependency() {
        // A reference to a fixer absent from the build (e.g. feature-gated)
        // is ignored rather than fatal.
        let a = detector_reg("fixer-a", &["fixer-nonexistent"], &[]);
        let sorted = topologically_sort_detectors(vec![&a]);
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].name, "fixer-a");
    }

    #[test]
    fn test_topological_sort_ignores_missing_before_dependency() {
        let a = detector_reg("fixer-a", &[], &["fixer-missing"]);
        let sorted = topologically_sort_detectors(vec![&a]);
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].name, "fixer-a");
    }

    /// A detector that yields a fixed list of diagnostics. Used by the
    /// apply-pipeline tests below.
    struct DiagDetector {
        name: &'static str,
        tags: &'static [&'static str],
        diagnostics: Vec<Diagnostic>,
    }

    impl Detector for DiagDetector {
        fn name(&self) -> &'static str {
            self.name
        }
        fn lintian_tags(&self) -> &'static [&'static str] {
            self.tags
        }
        fn detect(
            &self,
            _ws: &dyn Workspace,
            _preferences: &FixerPreferences,
        ) -> Result<Vec<Diagnostic>, FixerError> {
            Ok(self.diagnostics.clone())
        }
    }

    fn write_control(dir: &Path, content: &str) {
        let debian = dir.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), content).unwrap();
    }

    #[test]
    fn apply_pipeline_runs_diagnostic_actions() {
        let tmp = TempDir::new().unwrap();
        write_control(tmp.path(), "Source: foo\n\nPackage: foo\n");

        let detector = DiagDetector {
            name: "set-priority",
            tags: &["recommended-field"],
            diagnostics: vec![Diagnostic::with_actions(
                LintianIssue::source("recommended-field", Visibility::Warning),
                "Set Priority on source",
                "Set Priority on source",
                vec![Action::Deb822(Deb822Action::SetField {
                    file: PathBuf::from("debian/control"),
                    paragraph: ParagraphSelector::Source,
                    field: "Priority".into(),
                    value: "optional".into(),
                })],
            )
            .with_certainty(Certainty::Confident)],
        };
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("foo".into()),
            Some(version),
        );
        let result = detector.apply(&ws, &FixerPreferences::default()).unwrap();

        assert_eq!(result.description, "Set Priority on source");
        assert_eq!(result.certainty, Some(Certainty::Confident));
        assert_eq!(result.fixed_lintian_tags(), vec!["recommended-field"]);
        assert!(result.overridden_lintian_issues.is_empty());

        let after = fs::read_to_string(tmp.path().join("debian/control")).unwrap();
        assert_eq!(after, "Source: foo\nPriority: optional\n\nPackage: foo\n");
    }

    #[test]
    fn apply_pipeline_returns_no_changes_for_empty_diagnostics() {
        let tmp = TempDir::new().unwrap();
        write_control(tmp.path(), "Source: foo\n\nPackage: foo\n");

        let detector = DiagDetector {
            name: "noop",
            tags: &["x"],
            diagnostics: vec![],
        };
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("foo".into()),
            Some(version),
        );
        let err = detector
            .apply(&ws, &FixerPreferences::default())
            .unwrap_err();
        assert!(matches!(err, FixerError::NoChanges));
    }

    #[test]
    fn apply_pipeline_skips_overridden_diagnostics() {
        let tmp = TempDir::new().unwrap();
        write_control(tmp.path(), "Source: foo\n\nPackage: foo\n");
        // Override that suppresses the only diagnostic.
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("lintian-overrides"), "recommended-field\n").unwrap();

        let detector = DiagDetector {
            name: "set-priority",
            tags: &["recommended-field"],
            diagnostics: vec![Diagnostic::with_actions(
                LintianIssue::source("recommended-field", Visibility::Warning),
                "Priority field is missing on source.",
                "Set Priority on source",
                vec![Action::Deb822(Deb822Action::SetField {
                    file: PathBuf::from("debian/control"),
                    paragraph: ParagraphSelector::Source,
                    field: "Priority".into(),
                    value: "optional".into(),
                })],
            )],
        };
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("foo".into()),
            Some(version),
        );
        let err = detector
            .apply(&ws, &FixerPreferences::default())
            .unwrap_err();
        match err {
            FixerError::NoChangesAfterOverrides(issues) => {
                assert_eq!(issues.len(), 1);
                assert_eq!(issues[0].tag.as_deref(), Some("recommended-field"));
            }
            other => panic!("expected NoChangesAfterOverrides, got {:?}", other),
        }
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\n"
        );
    }

    #[test]
    fn apply_pipeline_filters_below_minimum_certainty() {
        let tmp = TempDir::new().unwrap();
        write_control(tmp.path(), "Source: foo\n\nPackage: foo\n");

        let detector = DiagDetector {
            name: "set-priority",
            tags: &["recommended-field"],
            diagnostics: vec![Diagnostic::with_actions(
                LintianIssue::source("recommended-field", Visibility::Warning),
                "Priority field is missing on source.",
                "Set Priority on source",
                vec![Action::Deb822(Deb822Action::SetField {
                    file: PathBuf::from("debian/control"),
                    paragraph: ParagraphSelector::Source,
                    field: "Priority".into(),
                    value: "optional".into(),
                })],
            )
            .with_certainty(Certainty::Possible)],
        };
        let mut prefs = FixerPreferences::default();
        prefs.minimum_certainty = Some(Certainty::Confident);
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("foo".into()),
            Some(version),
        );
        let err = detector.apply(&ws, &prefs).unwrap_err();
        assert!(matches!(err, FixerError::NotCertainEnough(..)));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\n"
        );
    }

    /// A detector that panics, used to confirm `detect_and_fix`
    /// catches the panic and converts it to `FixerError::Panic`.
    struct PanickyDetector;

    impl Detector for PanickyDetector {
        fn name(&self) -> &'static str {
            "panicky-detector"
        }
        fn lintian_tags(&self) -> &'static [&'static str] {
            &["x"]
        }
        fn detect(
            &self,
            _ws: &dyn Workspace,
            _preferences: &FixerPreferences,
        ) -> Result<Vec<Diagnostic>, FixerError> {
            panic!("Test panic from detector");
        }
    }

    #[test]
    fn detect_and_fix_catches_panic() {
        let tmp = TempDir::new().unwrap();
        let prefs = FixerPreferences::default();
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("test-package".into()),
            Some(version),
        );
        let result = detect_and_fix(&PanickyDetector, &ws, &prefs);
        match result.unwrap_err() {
            FixerError::Panic { message, .. } => {
                assert_eq!(message, "Test panic from detector");
            }
            other => panic!("expected FixerError::Panic, got {:?}", other),
        }
    }
}
