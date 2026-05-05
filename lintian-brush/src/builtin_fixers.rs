use super::*;

/// Registration information for a builtin fixer
pub struct BuiltinFixerRegistration {
    /// Name of the fixer
    pub name: &'static str,
    /// Lintian tags this fixer addresses
    pub lintian_tags: &'static [&'static str],
    /// Function to create an instance of the fixer
    pub create: fn() -> Box<dyn BuiltinFixer>,
    /// Fixers that must run before this one
    pub after: &'static [&'static str],
    /// Fixers that must run after this one
    pub before: &'static [&'static str],
}

inventory::collect!(BuiltinFixerRegistration);

/// Trait for implementing a builtin fixer.
///
/// Each fixer implements [`diagnostics`](Self::diagnostics) (the
/// detector). The framework drops any diagnostic that is overridden by
/// a lintian override or whose certainty is below
/// `preferences.minimum_certainty`, then applies the first plan of each
/// surviving diagnostic via [`crate::appliers`]. The
/// [`describe`](Self::describe) method controls the resulting commit
/// message.
pub trait BuiltinFixer: Send + Sync {
    /// Name of the fixer
    fn name(&self) -> &'static str;

    /// Lintian tags this fixer addresses
    fn lintian_tags(&self) -> &'static [&'static str];

    /// Detect issues without modifying the tree.
    fn diagnostics(
        &self,
        basedir: &std::path::Path,
        package: &str,
        current_version: &Version,
        preferences: &FixerPreferences,
    ) -> Result<Vec<crate::diagnostic::Diagnostic>, FixerError>;

    /// Build the commit message from the diagnostics that actually
    /// fired and the actions that were applied. The default
    /// deduplicates and joins the per-diagnostic messages; override
    /// when the description is a function of the *set* of issues or
    /// fields touched (e.g. "Set priority for library packages X, Y to
    /// optional.").
    fn describe(
        &self,
        fixed: &[crate::diagnostic::Diagnostic],
        actions: &[crate::diagnostic::Action],
    ) -> String {
        default_describe(fixed, actions)
    }

    /// Apply the fixer. Returns [`FixerError::NoChanges`] if no
    /// diagnostics were emitted, and [`FixerError::NoChangesAfterOverrides`]
    /// if every emitted diagnostic was filtered out by overrides.
    fn apply(
        &self,
        basedir: &std::path::Path,
        package: &str,
        current_version: &Version,
        preferences: &FixerPreferences,
    ) -> Result<FixerResult, FixerError> {
        let diagnostics = self.diagnostics(basedir, package, current_version, preferences)?;
        apply_diagnostics_with(basedir, &diagnostics, preferences, &|fixed, actions| {
            self.describe(fixed, actions)
        })
    }
}

/// Default describer used by [`BuiltinFixer::describe`] and
/// [`apply_diagnostics`].
///
/// Deduplicates the diagnostics' per-issue messages and joins them with
/// newlines.
pub fn default_describe(
    fixed: &[crate::diagnostic::Diagnostic],
    _actions: &[crate::diagnostic::Action],
) -> String {
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<&str> = fixed
        .iter()
        .map(|d| d.message.as_str())
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

/// Like [`apply_diagnostics`], but lets the caller provide a custom
/// describer. The describer receives the diagnostics that actually fired
/// (after override / certainty filtering) and the flat list of actions
/// that were applied, and must return the description string used in the
/// resulting [`FixerResult`].
pub fn apply_diagnostics_with(
    basedir: &std::path::Path,
    diagnostics: &[crate::diagnostic::Diagnostic],
    preferences: &FixerPreferences,
    describe: &dyn Fn(&[crate::diagnostic::Diagnostic], &[crate::diagnostic::Action]) -> String,
) -> Result<FixerResult, FixerError> {
    use debian_analyzer::certainty_sufficient;

    if diagnostics.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let min_certainty = preferences.minimum_certainty;

    let mut fixed: Vec<crate::diagnostic::Diagnostic> = Vec::new();
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
        let actual_certainty = diag.certainty.unwrap_or(Certainty::Certain);
        if !certainty_sufficient(actual_certainty, min_certainty) {
            if let Some(issue) = &diag.issue {
                not_certain_enough.push(issue.clone());
            }
            continue;
        }
        let allow_opinionated = preferences.opinionated.unwrap_or(false);
        let Some(plan) = diag
            .plans
            .iter()
            .find(|p| !p.opinionated || allow_opinionated)
        else {
            continue;
        };
        all_actions.extend(plan.actions.iter().cloned());
        fixed.push(diag.clone());
        min_actual_certainty = match (min_actual_certainty, diag.certainty) {
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

    let changed = crate::appliers::apply_actions(basedir, &all_actions)?;
    if changed.is_empty() {
        // Detector said there was something to fix but applying produced no
        // observable change. Treat as NoChanges to avoid an empty commit.
        return Err(FixerError::NoChanges);
    }

    let description = describe(&fixed, &all_actions);
    let patch_name = fixed.iter().find_map(|d| d.patch_name.clone());
    let fixed_issues: Vec<LintianIssue> = fixed.into_iter().filter_map(|d| d.issue).collect();

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

/// Wrapper to adapt BuiltinFixer trait to Fixer trait
pub struct BuiltinFixerWrapper {
    fixer: Box<dyn BuiltinFixer>,
    name: &'static str,
    lintian_tags: Vec<&'static str>,
}

impl BuiltinFixerWrapper {
    /// Create a new BuiltinFixerWrapper
    pub fn new(fixer: Box<dyn BuiltinFixer>) -> Self {
        let name = fixer.name();
        let lintian_tags = fixer.lintian_tags().to_vec();
        Self {
            fixer,
            name,
            lintian_tags,
        }
    }
}

impl std::fmt::Debug for BuiltinFixerWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuiltinFixerWrapper")
            .field("name", &self.name)
            .field("lintian_tags", &self.lintian_tags)
            .finish()
    }
}

impl Fixer for BuiltinFixerWrapper {
    fn name(&self) -> String {
        self.name.to_string()
    }

    fn lintian_tags(&self) -> Vec<String> {
        self.lintian_tags.iter().map(|s| s.to_string()).collect()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn run(
        &self,
        basedir: &std::path::Path,
        package: &str,
        current_version: &Version,
        preferences: &FixerPreferences,
        _timeout: Option<chrono::Duration>,
    ) -> Result<FixerResult, FixerError> {
        // Set extra environment variables from preferences for native fixers
        let mut env_backup = Vec::new();
        if let Some(extra_env) = &preferences.extra_env {
            for (key, value) in extra_env {
                // Backup existing value
                env_backup.push((key.clone(), std::env::var(key).ok()));
                // Set new value
                std::env::set_var(key, value);
            }
        }

        // Run the fixer with panic handling
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.fixer
                .apply(basedir, package, current_version, preferences)
        }));

        // Restore environment variables
        for (key, old_value) in env_backup {
            if let Some(value) = old_value {
                std::env::set_var(&key, value);
            } else {
                std::env::remove_var(&key);
            }
        }

        // Handle panic or return result
        match result {
            Ok(r) => r,
            Err(panic_payload) => {
                // Extract panic message
                let message = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic payload".to_string()
                };

                // Capture backtrace
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
}

/// View of a registration sufficient for topological sorting.
///
/// Both legacy [`BuiltinFixerRegistration`] and
/// [`crate::workspace::DetectorRegistration`] implement this so they can be
/// sorted together.
trait OrderedRegistration {
    fn name(&self) -> &'static str;
    fn after(&self) -> &'static [&'static str];
    fn before(&self) -> &'static [&'static str];
}

impl OrderedRegistration for &BuiltinFixerRegistration {
    fn name(&self) -> &'static str {
        self.name
    }
    fn after(&self) -> &'static [&'static str] {
        self.after
    }
    fn before(&self) -> &'static [&'static str] {
        self.before
    }
}

impl OrderedRegistration for &crate::workspace::DetectorRegistration {
    fn name(&self) -> &'static str {
        self.name
    }
    fn after(&self) -> &'static [&'static str] {
        self.after
    }
    fn before(&self) -> &'static [&'static str] {
        self.before
    }
}

/// Topologically sort fixers based on their dependencies
///
/// This function resolves both `after` and `before` constraints into a unified
/// dependency graph and performs topological sorting using Kahn's algorithm.
///
/// # Panics
///
/// Panics if:
/// - A circular dependency is detected
/// - A fixer references a non-existent dependency
fn topologically_sort_fixers<T: OrderedRegistration + Clone>(registrations: Vec<T>) -> Vec<T> {
    use std::collections::{HashMap, HashSet, VecDeque};

    // Build a map of fixer names to registrations for quick lookup
    let name_to_reg: HashMap<&str, T> = registrations
        .iter()
        .map(|reg| (reg.name(), reg.clone()))
        .collect();

    // Validate that all dependencies exist
    for reg in &registrations {
        for dep in reg.after() {
            if !name_to_reg.contains_key(dep) {
                panic!(
                    "Fixer '{}' declares dependency on non-existent fixer '{}' in 'after' list",
                    reg.name(),
                    dep
                );
            }
        }
        for dep in reg.before() {
            if !name_to_reg.contains_key(dep) {
                panic!(
                    "Fixer '{}' declares dependency on non-existent fixer '{}' in 'before' list",
                    reg.name(),
                    dep
                );
            }
        }
    }

    // Build adjacency list and in-degree map
    // edge A -> B means "A must run before B"
    let mut adj_list: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();

    // Initialize structures
    for reg in &registrations {
        adj_list.entry(reg.name()).or_default();
        in_degree.entry(reg.name()).or_insert(0);
    }

    // Add edges from 'after' constraints
    // If B declares after: [A], then A -> B (A must run before B)
    for reg in &registrations {
        for dep in reg.after() {
            adj_list.entry(*dep).or_default().push(reg.name());
            *in_degree.entry(reg.name()).or_insert(0) += 1;
        }
    }

    // Add edges from 'before' constraints
    // If A declares before: [B], then A -> B (A must run before B)
    for reg in &registrations {
        for dep in reg.before() {
            adj_list.entry(reg.name()).or_default().push(*dep);
            *in_degree.entry(*dep).or_insert(0) += 1;
        }
    }

    // Kahn's algorithm for topological sort
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, &degree)| degree == 0)
        .map(|(&name, _)| name)
        .collect();

    // Sort queue for deterministic ordering
    let mut queue_vec: Vec<_> = queue.drain(..).collect();
    queue_vec.sort();
    queue.extend(queue_vec);

    let mut sorted = Vec::new();
    let mut processed = HashSet::new();

    while let Some(node) = queue.pop_front() {
        sorted.push(node);
        processed.insert(node);

        // Get neighbors and sort for deterministic ordering
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

        // Re-sort queue for deterministic ordering
        let mut queue_vec: Vec<_> = queue.drain(..).collect();
        queue_vec.sort();
        queue.extend(queue_vec);
    }

    // Check for cycles
    if sorted.len() != registrations.len() {
        // Find the cycle for error reporting
        let remaining: Vec<_> = registrations
            .iter()
            .filter(|reg| !processed.contains(reg.name()))
            .map(|reg| reg.name())
            .collect();

        // Build a detailed cycle description
        let mut cycle_msg = String::from("Circular dependency detected among fixers: ");
        cycle_msg.push_str(&remaining.join(", "));
        cycle_msg.push_str("\nDependency relationships:");

        for name in &remaining {
            if let Some(reg) = name_to_reg.get(name) {
                if !reg.after().is_empty() {
                    cycle_msg.push_str(&format!(
                        "\n  '{}' after: [{}]",
                        name,
                        reg.after().join(", ")
                    ));
                }
                if !reg.before().is_empty() {
                    cycle_msg.push_str(&format!(
                        "\n  '{}' before: [{}]",
                        name,
                        reg.before().join(", ")
                    ));
                }
            }
        }

        panic!("{}", cycle_msg);
    }

    // Convert sorted names back to registrations
    sorted
        .iter()
        .map(|name| name_to_reg[name].clone())
        .collect()
}

/// Construct a fixer instance for a sorted entry — either from a legacy
/// registration's `create` fn or by wrapping a freshly-created detector in
/// a [`crate::workspace::DetectorAdapter`].
#[derive(Clone)]
enum MergedRegistration {
    Legacy(&'static BuiltinFixerRegistration),
    Detector(&'static crate::workspace::DetectorRegistration),
}

impl OrderedRegistration for MergedRegistration {
    fn name(&self) -> &'static str {
        match self {
            MergedRegistration::Legacy(reg) => reg.name,
            MergedRegistration::Detector(reg) => reg.name,
        }
    }
    fn after(&self) -> &'static [&'static str] {
        match self {
            MergedRegistration::Legacy(reg) => reg.after,
            MergedRegistration::Detector(reg) => reg.after,
        }
    }
    fn before(&self) -> &'static [&'static str] {
        match self {
            MergedRegistration::Legacy(reg) => reg.before,
            MergedRegistration::Detector(reg) => reg.before,
        }
    }
}

impl MergedRegistration {
    fn into_fixer(self) -> Box<dyn Fixer> {
        match self {
            MergedRegistration::Legacy(reg) => {
                Box::new(BuiltinFixerWrapper::new((reg.create)())) as Box<dyn Fixer>
            }
            MergedRegistration::Detector(reg) => {
                let adapter = crate::workspace::DetectorAdapter::new((reg.create)());
                Box::new(BuiltinFixerWrapper::new(Box::new(adapter))) as Box<dyn Fixer>
            }
        }
    }
}

/// Get all registered builtin fixers.
///
/// Yields fixers from both the legacy `BuiltinFixer` inventory and the
/// modern [`crate::workspace::Detector`] inventory. Detectors are wrapped
/// in [`crate::workspace::DetectorAdapter`] so they look like ordinary
/// `BuiltinFixer`s to the CLI driver. Both kinds are sorted together so
/// `after`/`before` declarations can cross the legacy/detector boundary.
pub fn get_builtin_fixers() -> Vec<Box<dyn Fixer>> {
    let mut merged: Vec<MergedRegistration> = Vec::new();
    merged.extend(
        inventory::iter::<BuiltinFixerRegistration>
            .into_iter()
            .map(MergedRegistration::Legacy),
    );
    merged.extend(
        inventory::iter::<crate::workspace::DetectorRegistration>
            .into_iter()
            .map(MergedRegistration::Detector),
    );

    let sorted = topologically_sort_fixers(merged);
    sorted
        .into_iter()
        .map(MergedRegistration::into_fixer)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // Mock builtin fixer for testing
    struct MockBuiltinFixer {
        name: &'static str,
        tags: &'static [&'static str],
    }

    impl BuiltinFixer for MockBuiltinFixer {
        fn name(&self) -> &'static str {
            self.name
        }

        fn lintian_tags(&self) -> &'static [&'static str] {
            self.tags
        }

        fn diagnostics(
            &self,
            _basedir: &Path,
            _package: &str,
            _current_version: &Version,
            _preferences: &FixerPreferences,
        ) -> Result<Vec<crate::diagnostic::Diagnostic>, FixerError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn test_builtin_fixers_dependency_consistency() {
        // This test verifies that all builtin fixers have consistent dependencies:
        // 1. No circular dependencies
        // 2. All referenced fixers in after/before actually exist
        // 3. All registered fixers are successfully sorted (none lost)
        //
        // The topological sort will panic if there are issues, which fails this test.

        let mut all_registrations: Vec<MergedRegistration> = Vec::new();
        all_registrations.extend(
            inventory::iter::<BuiltinFixerRegistration>
                .into_iter()
                .map(MergedRegistration::Legacy),
        );
        all_registrations.extend(
            inventory::iter::<crate::workspace::DetectorRegistration>
                .into_iter()
                .map(MergedRegistration::Detector),
        );

        let original_count = all_registrations.len();

        // This will panic if there are circular dependencies or missing references
        let sorted = topologically_sort_fixers(all_registrations.clone());

        // Verify no fixers were lost during sorting
        assert_eq!(
            sorted.len(),
            original_count,
            "Topological sort lost some fixers! Expected {}, got {}",
            original_count,
            sorted.len()
        );

        // Verify all fixers are unique in the output
        let mut seen_names = std::collections::HashSet::new();
        for reg in &sorted {
            assert!(
                seen_names.insert(reg.name()),
                "Duplicate fixer name in sorted output: {}",
                reg.name()
            );
        }

        // Verify dependencies are satisfied in the sorted order
        let name_to_index: std::collections::HashMap<_, _> = sorted
            .iter()
            .enumerate()
            .map(|(idx, reg)| (reg.name(), idx))
            .collect();

        for (idx, reg) in sorted.iter().enumerate() {
            // Check that all 'after' dependencies come before this fixer
            for dep in reg.after() {
                let dep_idx = name_to_index.get(dep).expect(&format!(
                    "Fixer '{}' declares after: ['{}'], but '{}' not found in sorted output",
                    reg.name(),
                    dep,
                    dep
                ));
                assert!(
                    dep_idx < &idx,
                    "Dependency ordering violated: '{}' (index {}) should run after '{}' (index {}), but doesn't",
                    reg.name(), idx, dep, dep_idx
                );
            }

            // Check that all 'before' dependencies come after this fixer
            for dep in reg.before() {
                let dep_idx = name_to_index.get(dep).expect(&format!(
                    "Fixer '{}' declares before: ['{}'], but '{}' not found in sorted output",
                    reg.name(),
                    dep,
                    dep
                ));
                assert!(
                    dep_idx > &idx,
                    "Dependency ordering violated: '{}' (index {}) should run before '{}' (index {}), but doesn't",
                    reg.name(), idx, dep, dep_idx
                );
            }
        }
    }

    #[test]
    fn test_get_builtin_fixers() {
        let fixers = get_builtin_fixers();
        // Check that we have at least two fixers now
        assert!(
            fixers.len() >= 2,
            "Expected at least 2 builtin fixers, found {}",
            fixers.len()
        );

        // Check that the CRLF fixer is registered
        let crlf_fixer = fixers
            .iter()
            .find(|f| f.name() == "control-file-with-CRLF-EOLs");
        assert!(crlf_fixer.is_some(), "CRLF fixer not found");

        // Check that the executable desktop file fixer is registered
        let desktop_fixer = fixers
            .iter()
            .find(|f| f.name() == "executable-desktop-file");
        assert!(
            desktop_fixer.is_some(),
            "executable-desktop-file fixer not found"
        );
    }

    #[test]
    fn test_builtin_fixer_wrapper_new() {
        let mock_fixer = MockBuiltinFixer {
            name: "test-fixer",
            tags: &["test-tag1", "test-tag2"],
        };

        let wrapper = BuiltinFixerWrapper::new(Box::new(mock_fixer));

        assert_eq!(wrapper.name, "test-fixer");
        assert_eq!(wrapper.lintian_tags, vec!["test-tag1", "test-tag2"]);
    }

    #[test]
    fn test_builtin_fixer_wrapper_fixer_trait() {
        let mock_fixer = MockBuiltinFixer {
            name: "test-fixer",
            tags: &["test-tag"],
        };

        let wrapper = BuiltinFixerWrapper::new(Box::new(mock_fixer));
        let fixer: &dyn Fixer = &wrapper;

        assert_eq!(fixer.name(), "test-fixer");
        assert_eq!(fixer.lintian_tags(), vec!["test-tag"]);
    }

    #[test]
    fn test_builtin_fixer_wrapper_run() {
        // The mock fixer emits no diagnostics, so apply returns
        // NoChanges. We just check the wrapper plumbs through to the
        // underlying BuiltinFixer.
        let mock_fixer = MockBuiltinFixer {
            name: "test-fixer",
            tags: &["test-tag"],
        };

        let wrapper = BuiltinFixerWrapper::new(Box::new(mock_fixer));
        let temp_dir = tempfile::tempdir().unwrap();
        let preferences = FixerPreferences::default();
        let version: Version = "1.0".parse().unwrap();

        let result = wrapper.run(
            temp_dir.path(),
            "test-package",
            &version,
            &preferences,
            None,
        );

        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_builtin_fixer_wrapper_as_any() {
        let mock_fixer = MockBuiltinFixer {
            name: "test-fixer",
            tags: &[],
        };

        let wrapper = BuiltinFixerWrapper::new(Box::new(mock_fixer));
        let fixer: &dyn Fixer = &wrapper;

        // Test that as_any() works
        let any = fixer.as_any();
        assert!(any.downcast_ref::<BuiltinFixerWrapper>().is_some());
    }

    #[test]
    fn test_builtin_fixer_wrapper_debug() {
        let mock_fixer = MockBuiltinFixer {
            name: "test-fixer",
            tags: &["tag1", "tag2"],
        };

        let wrapper = BuiltinFixerWrapper::new(Box::new(mock_fixer));
        let debug_str = format!("{:?}", wrapper);

        assert!(debug_str.contains("BuiltinFixerWrapper"));
        assert!(debug_str.contains("test-fixer"));
        assert!(debug_str.contains("tag1"));
        assert!(debug_str.contains("tag2"));
    }

    // Mock builtin fixer that panics
    struct PanicBuiltinFixer {
        name: &'static str,
        tags: &'static [&'static str],
    }

    impl BuiltinFixer for PanicBuiltinFixer {
        fn name(&self) -> &'static str {
            self.name
        }

        fn lintian_tags(&self) -> &'static [&'static str] {
            self.tags
        }

        fn diagnostics(
            &self,
            _basedir: &Path,
            _package: &str,
            _current_version: &Version,
            _preferences: &FixerPreferences,
        ) -> Result<Vec<crate::diagnostic::Diagnostic>, FixerError> {
            panic!("Test panic from fixer");
        }
    }

    #[test]
    fn test_builtin_fixer_wrapper_catches_panic() {
        let panic_fixer = PanicBuiltinFixer {
            name: "panic-test-fixer",
            tags: &["test-tag"],
        };

        let wrapper = BuiltinFixerWrapper::new(Box::new(panic_fixer));
        let temp_dir = tempfile::tempdir().unwrap();
        let preferences = FixerPreferences::default();
        let version: Version = "1.0".parse().unwrap();

        let result = wrapper.run(
            temp_dir.path(),
            "test-package",
            &version,
            &preferences,
            None,
        );

        // Verify that the panic was caught and converted to an error
        assert!(result.is_err());
        let err = result.unwrap_err();

        // Check that it's a Panic variant
        match err {
            FixerError::Panic {
                message,
                backtrace: _,
            } => {
                assert_eq!(message, "Test panic from fixer");
            }
            _ => panic!("Expected FixerError::Panic, got {:?}", err),
        }
    }

    // Tests for topological sorting and dependency resolution
    #[test]
    fn test_topological_sort_no_dependencies() {
        let reg1 = BuiltinFixerRegistration {
            name: "fixer-a",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-a",
                    tags: &[],
                })
            },
            after: &[],
            before: &[],
        };
        let reg2 = BuiltinFixerRegistration {
            name: "fixer-b",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-b",
                    tags: &[],
                })
            },
            after: &[],
            before: &[],
        };
        let reg3 = BuiltinFixerRegistration {
            name: "fixer-c",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-c",
                    tags: &[],
                })
            },
            after: &[],
            before: &[],
        };

        let registrations = vec![&reg1, &reg2, &reg3];
        let sorted = topologically_sort_fixers(registrations);

        // Should be sorted alphabetically when no dependencies
        assert_eq!(sorted[0].name, "fixer-a");
        assert_eq!(sorted[1].name, "fixer-b");
        assert_eq!(sorted[2].name, "fixer-c");
    }

    #[test]
    fn test_topological_sort_simple_after() {
        let reg1 = BuiltinFixerRegistration {
            name: "fixer-a",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-a",
                    tags: &[],
                })
            },
            after: &[],
            before: &[],
        };
        let reg2 = BuiltinFixerRegistration {
            name: "fixer-b",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-b",
                    tags: &[],
                })
            },
            after: &["fixer-a"], // B runs after A
            before: &[],
        };

        let registrations = vec![&reg2, &reg1]; // Intentionally out of order
        let sorted = topologically_sort_fixers(registrations);

        // A should come before B
        assert_eq!(sorted[0].name, "fixer-a");
        assert_eq!(sorted[1].name, "fixer-b");
    }

    #[test]
    fn test_topological_sort_simple_before() {
        let reg1 = BuiltinFixerRegistration {
            name: "fixer-a",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-a",
                    tags: &[],
                })
            },
            after: &[],
            before: &["fixer-b"], // A runs before B
        };
        let reg2 = BuiltinFixerRegistration {
            name: "fixer-b",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-b",
                    tags: &[],
                })
            },
            after: &[],
            before: &[],
        };

        let registrations = vec![&reg2, &reg1]; // Intentionally out of order
        let sorted = topologically_sort_fixers(registrations);

        // A should come before B
        assert_eq!(sorted[0].name, "fixer-a");
        assert_eq!(sorted[1].name, "fixer-b");
    }

    #[test]
    fn test_topological_sort_chain() {
        let reg1 = BuiltinFixerRegistration {
            name: "fixer-a",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-a",
                    tags: &[],
                })
            },
            after: &[],
            before: &[],
        };
        let reg2 = BuiltinFixerRegistration {
            name: "fixer-b",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-b",
                    tags: &[],
                })
            },
            after: &["fixer-a"],
            before: &[],
        };
        let reg3 = BuiltinFixerRegistration {
            name: "fixer-c",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-c",
                    tags: &[],
                })
            },
            after: &["fixer-b"],
            before: &[],
        };

        let registrations = vec![&reg3, &reg1, &reg2]; // Scrambled order
        let sorted = topologically_sort_fixers(registrations);

        // Should be A -> B -> C
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
        let reg_a = BuiltinFixerRegistration {
            name: "fixer-a",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-a",
                    tags: &[],
                })
            },
            after: &[],
            before: &["fixer-b", "fixer-c"],
        };
        let reg_b = BuiltinFixerRegistration {
            name: "fixer-b",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-b",
                    tags: &[],
                })
            },
            after: &["fixer-a"],
            before: &["fixer-d"],
        };
        let reg_c = BuiltinFixerRegistration {
            name: "fixer-c",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-c",
                    tags: &[],
                })
            },
            after: &["fixer-a"],
            before: &["fixer-d"],
        };
        let reg_d = BuiltinFixerRegistration {
            name: "fixer-d",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-d",
                    tags: &[],
                })
            },
            after: &["fixer-b", "fixer-c"],
            before: &[],
        };

        let registrations = vec![&reg_d, &reg_c, &reg_b, &reg_a];
        let sorted = topologically_sort_fixers(registrations);

        // A must be first, D must be last
        assert_eq!(sorted[0].name, "fixer-a");
        assert_eq!(sorted[3].name, "fixer-d");
        // B and C can be in either order (both depend on A and come before D)
        let middle_names: Vec<_> = sorted[1..3].iter().map(|r| r.name).collect();
        assert!(middle_names.contains(&"fixer-b"));
        assert!(middle_names.contains(&"fixer-c"));
    }

    #[test]
    #[should_panic(expected = "Circular dependency detected")]
    fn test_topological_sort_circular_dependency_simple() {
        let reg1 = BuiltinFixerRegistration {
            name: "fixer-a",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-a",
                    tags: &[],
                })
            },
            after: &["fixer-b"], // A after B
            before: &[],
        };
        let reg2 = BuiltinFixerRegistration {
            name: "fixer-b",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-b",
                    tags: &[],
                })
            },
            after: &["fixer-a"], // B after A (cycle!)
            before: &[],
        };

        let registrations = vec![&reg1, &reg2];
        topologically_sort_fixers(registrations); // Should panic
    }

    #[test]
    #[should_panic(expected = "Circular dependency detected")]
    fn test_topological_sort_circular_dependency_complex() {
        // A -> B -> C -> A (cycle)
        let reg_a = BuiltinFixerRegistration {
            name: "fixer-a",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-a",
                    tags: &[],
                })
            },
            after: &[],
            before: &["fixer-b"],
        };
        let reg_b = BuiltinFixerRegistration {
            name: "fixer-b",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-b",
                    tags: &[],
                })
            },
            after: &["fixer-a"],
            before: &["fixer-c"],
        };
        let reg_c = BuiltinFixerRegistration {
            name: "fixer-c",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-c",
                    tags: &[],
                })
            },
            after: &["fixer-b"],
            before: &["fixer-a"], // Creates cycle
        };

        let registrations = vec![&reg_a, &reg_b, &reg_c];
        topologically_sort_fixers(registrations); // Should panic
    }

    #[test]
    #[should_panic(expected = "non-existent fixer 'fixer-nonexistent'")]
    fn test_topological_sort_missing_dependency_after() {
        let reg = BuiltinFixerRegistration {
            name: "fixer-a",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-a",
                    tags: &[],
                })
            },
            after: &["fixer-nonexistent"], // References non-existent fixer
            before: &[],
        };

        let registrations = vec![&reg];
        topologically_sort_fixers(registrations); // Should panic
    }

    #[test]
    #[should_panic(expected = "non-existent fixer 'fixer-missing'")]
    fn test_topological_sort_missing_dependency_before() {
        let reg = BuiltinFixerRegistration {
            name: "fixer-a",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-a",
                    tags: &[],
                })
            },
            after: &[],
            before: &["fixer-missing"], // References non-existent fixer
        };

        let registrations = vec![&reg];
        topologically_sort_fixers(registrations); // Should panic
    }

    #[test]
    fn test_topological_sort_mixed_after_before() {
        // A before B, B after A (both constraints point same direction)
        let reg_a = BuiltinFixerRegistration {
            name: "fixer-a",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-a",
                    tags: &[],
                })
            },
            after: &[],
            before: &["fixer-b"],
        };
        let reg_b = BuiltinFixerRegistration {
            name: "fixer-b",
            lintian_tags: &[],
            create: || {
                Box::new(MockBuiltinFixer {
                    name: "fixer-b",
                    tags: &[],
                })
            },
            after: &["fixer-a"],
            before: &[],
        };

        let registrations = vec![&reg_b, &reg_a];
        let sorted = topologically_sort_fixers(registrations);

        // A should come before B
        assert_eq!(sorted[0].name, "fixer-a");
        assert_eq!(sorted[1].name, "fixer-b");
    }

    use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// A fixer that overrides only `diagnostics`. The default `apply` impl
    /// from the trait should consume the diagnostics, filter via overrides,
    /// and apply the actions.
    struct DiagFixer {
        name: &'static str,
        tags: &'static [&'static str],
        diagnostics: Vec<Diagnostic>,
    }

    impl BuiltinFixer for DiagFixer {
        fn name(&self) -> &'static str {
            self.name
        }
        fn lintian_tags(&self) -> &'static [&'static str] {
            self.tags
        }
        fn diagnostics(
            &self,
            _basedir: &Path,
            _package: &str,
            _version: &Version,
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
    fn default_apply_runs_diagnostic_actions() {
        let tmp = TempDir::new().unwrap();
        write_control(tmp.path(), "Source: foo\n\nPackage: foo\n");

        let fixer = DiagFixer {
            name: "set-priority",
            tags: &["recommended-field"],
            diagnostics: vec![Diagnostic::with_actions(
                LintianIssue::source("recommended-field"),
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
        let result = fixer
            .apply(tmp.path(), "foo", &version, &FixerPreferences::default())
            .unwrap();

        assert_eq!(result.description, "Set Priority on source");
        assert_eq!(result.certainty, Some(Certainty::Confident));
        assert_eq!(result.fixed_lintian_tags(), vec!["recommended-field"]);
        assert!(result.overridden_lintian_issues.is_empty());

        let after = fs::read_to_string(tmp.path().join("debian/control")).unwrap();
        assert_eq!(after, "Source: foo\nPriority: optional\n\nPackage: foo\n");
    }

    #[test]
    fn default_apply_returns_no_changes_for_empty_diagnostics() {
        let tmp = TempDir::new().unwrap();
        write_control(tmp.path(), "Source: foo\n\nPackage: foo\n");

        let fixer = DiagFixer {
            name: "noop",
            tags: &["x"],
            diagnostics: vec![],
        };
        let version: Version = "1.0".parse().unwrap();
        let err = fixer
            .apply(tmp.path(), "foo", &version, &FixerPreferences::default())
            .unwrap_err();
        assert!(matches!(err, FixerError::NoChanges));
    }

    #[test]
    fn default_apply_skips_overridden_diagnostics() {
        let tmp = TempDir::new().unwrap();
        write_control(tmp.path(), "Source: foo\n\nPackage: foo\n");
        // Override that suppresses the only diagnostic.
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("lintian-overrides"), "recommended-field\n").unwrap();

        let fixer = DiagFixer {
            name: "set-priority",
            tags: &["recommended-field"],
            diagnostics: vec![Diagnostic::with_actions(
                LintianIssue::source("recommended-field"),
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
        let err = fixer
            .apply(tmp.path(), "foo", &version, &FixerPreferences::default())
            .unwrap_err();
        match err {
            FixerError::NoChangesAfterOverrides(issues) => {
                assert_eq!(issues.len(), 1);
                assert_eq!(issues[0].tag.as_deref(), Some("recommended-field"));
            }
            other => panic!("expected NoChangesAfterOverrides, got {:?}", other),
        }
        // Control file untouched.
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\n"
        );
    }

    #[test]
    fn default_apply_filters_below_minimum_certainty() {
        let tmp = TempDir::new().unwrap();
        write_control(tmp.path(), "Source: foo\n\nPackage: foo\n");

        let fixer = DiagFixer {
            name: "set-priority",
            tags: &["recommended-field"],
            diagnostics: vec![Diagnostic::with_actions(
                LintianIssue::source("recommended-field"),
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
        let err = fixer
            .apply(tmp.path(), "foo", &version, &prefs)
            .unwrap_err();
        assert!(matches!(err, FixerError::NotCertainEnough(..)));
        // Control file untouched.
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\n"
        );
    }
}
