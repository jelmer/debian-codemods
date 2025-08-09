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
#[macro_export]
macro_rules! declare_fixer {
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
            }
        }
    };
}
