/// Macro to declare a builtin fixer.
///
/// The fixer's detector returns `Vec<Diagnostic>`; the framework
/// filters by lintian overrides and certainty, then applies actions.
///
/// # Example
/// ```ignore
/// declare_fixer! {
///     name: "my-fixer",
///     tags: ["my-lintian-tag"],
///     diagnose: |basedir, _package, _version, _preferences| {
///         // returns Result<Vec<Diagnostic>, FixerError>
///     }
/// }
/// ```
///
/// Optional `after: [...]` and/or `before: [...]` clauses set ordering
/// constraints. An optional `describe: |fixed, actions| { ... }` clause
/// overrides the default per-diagnostic message join.
#[macro_export]
macro_rules! declare_fixer {
    // Full form: diagnose + describe + after + before.
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
        after: [$($after:expr),*],
        before: [$($before:expr),*],
        diagnose: $diagnose_fn:expr,
        describe: $describe_fn:expr
    ) => {
        struct FixerImpl;

        impl $crate::builtin_fixers::BuiltinFixer for FixerImpl {
            fn name(&self) -> &'static str {
                $name
            }

            fn lintian_tags(&self) -> &'static [&'static str] {
                &[$($tag),*]
            }

            fn diagnostics(
                &self,
                basedir: &std::path::Path,
                package: &str,
                current_version: &$crate::Version,
                preferences: &$crate::FixerPreferences,
            ) -> Result<Vec<$crate::diagnostic::Diagnostic>, $crate::FixerError> {
                let diagnose_fn: fn(
                    &std::path::Path,
                    &str,
                    &$crate::Version,
                    &$crate::FixerPreferences,
                ) -> Result<Vec<$crate::diagnostic::Diagnostic>, $crate::FixerError> =
                    $diagnose_fn;
                diagnose_fn(basedir, package, current_version, preferences)
            }

            fn describe(
                &self,
                fixed: &[$crate::diagnostic::Diagnostic],
                actions: &[$crate::diagnostic::Action],
            ) -> String {
                let describe_fn: fn(
                    &[$crate::diagnostic::Diagnostic],
                    &[$crate::diagnostic::Action],
                ) -> String = $describe_fn;
                describe_fn(fixed, actions)
            }
        }

        $crate::inventory::submit! {
            $crate::builtin_fixers::BuiltinFixerRegistration {
                name: $name,
                lintian_tags: &[$($tag),*],
                create: || Box::new(FixerImpl),
                after: &[$($after),*],
                before: &[$($before),*],
            }
        }
    };

    // Diagnose + describe, after only.
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
        after: [$($after:expr),*],
        diagnose: $diagnose_fn:expr,
        describe: $describe_fn:expr
    ) => {
        $crate::declare_fixer! {
            name: $name,
            tags: [$($tag),*],
            after: [$($after),*],
            before: [],
            diagnose: $diagnose_fn,
            describe: $describe_fn
        }
    };

    // Diagnose + describe, before only.
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
        before: [$($before:expr),*],
        diagnose: $diagnose_fn:expr,
        describe: $describe_fn:expr
    ) => {
        $crate::declare_fixer! {
            name: $name,
            tags: [$($tag),*],
            after: [],
            before: [$($before),*],
            diagnose: $diagnose_fn,
            describe: $describe_fn
        }
    };

    // Diagnose + describe, no ordering.
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
        diagnose: $diagnose_fn:expr,
        describe: $describe_fn:expr
    ) => {
        $crate::declare_fixer! {
            name: $name,
            tags: [$($tag),*],
            after: [],
            before: [],
            diagnose: $diagnose_fn,
            describe: $describe_fn
        }
    };

    // Diagnose only, with after and before.
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
        after: [$($after:expr),*],
        before: [$($before:expr),*],
        diagnose: $diagnose_fn:expr
    ) => {
        struct FixerImpl;

        impl $crate::builtin_fixers::BuiltinFixer for FixerImpl {
            fn name(&self) -> &'static str {
                $name
            }

            fn lintian_tags(&self) -> &'static [&'static str] {
                &[$($tag),*]
            }

            fn diagnostics(
                &self,
                basedir: &std::path::Path,
                package: &str,
                current_version: &$crate::Version,
                preferences: &$crate::FixerPreferences,
            ) -> Result<Vec<$crate::diagnostic::Diagnostic>, $crate::FixerError> {
                let diagnose_fn: fn(
                    &std::path::Path,
                    &str,
                    &$crate::Version,
                    &$crate::FixerPreferences,
                ) -> Result<Vec<$crate::diagnostic::Diagnostic>, $crate::FixerError> =
                    $diagnose_fn;
                diagnose_fn(basedir, package, current_version, preferences)
            }
        }

        $crate::inventory::submit! {
            $crate::builtin_fixers::BuiltinFixerRegistration {
                name: $name,
                lintian_tags: &[$($tag),*],
                create: || Box::new(FixerImpl),
                after: &[$($after),*],
                before: &[$($before),*],
            }
        }
    };

    // Diagnose only, after only.
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
        after: [$($after:expr),*],
        diagnose: $diagnose_fn:expr
    ) => {
        $crate::declare_fixer! {
            name: $name,
            tags: [$($tag),*],
            after: [$($after),*],
            before: [],
            diagnose: $diagnose_fn
        }
    };

    // Diagnose only, before only.
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
        before: [$($before:expr),*],
        diagnose: $diagnose_fn:expr
    ) => {
        $crate::declare_fixer! {
            name: $name,
            tags: [$($tag),*],
            after: [],
            before: [$($before),*],
            diagnose: $diagnose_fn
        }
    };

    // Diagnose only, no ordering.
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
        diagnose: $diagnose_fn:expr
    ) => {
        $crate::declare_fixer! {
            name: $name,
            tags: [$($tag),*],
            after: [],
            before: [],
            diagnose: $diagnose_fn
        }
    };
}
