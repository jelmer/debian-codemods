/// Macro to declare a builtin fixer
///
/// This macro generates the necessary registration code for a builtin fixer.
///
/// # Example
/// ```
/// use lintian_brush::{declare_fixer, FixerError, FixerResult, FixerPreferences, Version, Certainty};
///
/// declare_fixer! {
///     name: "my-fixer",
///     tags: ["my-lintian-tag"],
///     apply: |_basedir, _package, _version, _preferences| {
///         Ok(FixerResult::builder("Fixed something")
///             .certainty(Certainty::Certain)
///             .build())
///     }
/// }
/// ```
///
/// # Example with dependencies
/// ```ignore
/// declare_fixer! {
///     name: "my-fixer",
///     tags: ["my-lintian-tag"],
///     after: ["other-fixer"],
///     before: ["another-fixer"],
///     apply: |_basedir, _package, _version, _preferences| {
///         Ok(FixerResult::builder("Fixed something")
///             .certainty(Certainty::Certain)
///             .build())
///     }
/// }
/// ```
#[macro_export]
macro_rules! declare_fixer {
    // Full form with both after and before
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
        after: [$($after:expr),*],
        before: [$($before:expr),*],
        apply: $apply_fn:expr
    ) => {
        struct FixerImpl;

        impl $crate::builtin_fixers::BuiltinFixer for FixerImpl {
            fn name(&self) -> &'static str {
                $name
            }

            fn lintian_tags(&self) -> &'static [&'static str] {
                &[$($tag),*]
            }

            fn apply(
                &self,
                basedir: &std::path::Path,
                package: &str,
                current_version: &$crate::Version,
                preferences: &$crate::FixerPreferences,
            ) -> Result<$crate::FixerResult, $crate::FixerError> {
                let apply_fn: fn(&std::path::Path, &str, &$crate::Version, &$crate::FixerPreferences) -> Result<$crate::FixerResult, $crate::FixerError> = $apply_fn;
                apply_fn(basedir, package, current_version, preferences)
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

    // With after only
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
        after: [$($after:expr),*],
        apply: $apply_fn:expr
    ) => {
        struct FixerImpl;

        impl $crate::builtin_fixers::BuiltinFixer for FixerImpl {
            fn name(&self) -> &'static str {
                $name
            }

            fn lintian_tags(&self) -> &'static [&'static str] {
                &[$($tag),*]
            }

            fn apply(
                &self,
                basedir: &std::path::Path,
                package: &str,
                current_version: &$crate::Version,
                preferences: &$crate::FixerPreferences,
            ) -> Result<$crate::FixerResult, $crate::FixerError> {
                let apply_fn: fn(&std::path::Path, &str, &$crate::Version, &$crate::FixerPreferences) -> Result<$crate::FixerResult, $crate::FixerError> = $apply_fn;
                apply_fn(basedir, package, current_version, preferences)
            }
        }

        $crate::inventory::submit! {
            $crate::builtin_fixers::BuiltinFixerRegistration {
                name: $name,
                lintian_tags: &[$($tag),*],
                create: || Box::new(FixerImpl),
                after: &[$($after),*],
                before: &[],
            }
        }
    };

    // With before only
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
        before: [$($before:expr),*],
        apply: $apply_fn:expr
    ) => {
        struct FixerImpl;

        impl $crate::builtin_fixers::BuiltinFixer for FixerImpl {
            fn name(&self) -> &'static str {
                $name
            }

            fn lintian_tags(&self) -> &'static [&'static str] {
                &[$($tag),*]
            }

            fn apply(
                &self,
                basedir: &std::path::Path,
                package: &str,
                current_version: &$crate::Version,
                preferences: &$crate::FixerPreferences,
            ) -> Result<$crate::FixerResult, $crate::FixerError> {
                let apply_fn: fn(&std::path::Path, &str, &$crate::Version, &$crate::FixerPreferences) -> Result<$crate::FixerResult, $crate::FixerError> = $apply_fn;
                apply_fn(basedir, package, current_version, preferences)
            }
        }

        $crate::inventory::submit! {
            $crate::builtin_fixers::BuiltinFixerRegistration {
                name: $name,
                lintian_tags: &[$($tag),*],
                create: || Box::new(FixerImpl),
                after: &[],
                before: &[$($before),*],
            }
        }
    };

    // Diagnostics + custom description, with both after and before.
    // Like the diagnose-only form below, but the message used in the
    // FixerResult is computed from the set of diagnostics that actually
    // fired rather than concatenated from each diagnostic's own message.
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

    // Diagnostics + custom description, with after only.
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

    // Diagnostics + custom description, with before only.
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

    // Diagnostics + custom description, no dependencies.
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

    // Diagnostics-only form with both after and before.
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

    // Diagnostics-only form with after only.
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

    // Diagnostics-only form with before only.
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

    // Diagnostics-only form. The detector returns Diagnostics; the default
    // BuiltinFixer::apply impl filters by overrides / minimum certainty and
    // applies the actions via crate::appliers.
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
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
                after: &[],
                before: &[],
            }
        }
    };

    // Original form without dependencies (for backward compatibility)
    (
        name: $name:expr,
        tags: [$($tag:expr),*],
        apply: $apply_fn:expr
    ) => {
        struct FixerImpl;

        impl $crate::builtin_fixers::BuiltinFixer for FixerImpl {
            fn name(&self) -> &'static str {
                $name
            }

            fn lintian_tags(&self) -> &'static [&'static str] {
                &[$($tag),*]
            }

            fn apply(
                &self,
                basedir: &std::path::Path,
                package: &str,
                current_version: &$crate::Version,
                preferences: &$crate::FixerPreferences,
            ) -> Result<$crate::FixerResult, $crate::FixerError> {
                let apply_fn: fn(&std::path::Path, &str, &$crate::Version, &$crate::FixerPreferences) -> Result<$crate::FixerResult, $crate::FixerError> = $apply_fn;
                apply_fn(basedir, package, current_version, preferences)
            }
        }

        $crate::inventory::submit! {
            $crate::builtin_fixers::BuiltinFixerRegistration {
                name: $name,
                lintian_tags: &[$($tag),*],
                create: || Box::new(FixerImpl),
                after: &[],
                before: &[],
            }
        }
    };
}
