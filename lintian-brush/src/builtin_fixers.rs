use super::*;

/// Registration information for a builtin fixer
pub struct BuiltinFixerRegistration {
    /// Name of the fixer
    pub name: &'static str,
    /// Lintian tags this fixer addresses
    pub lintian_tags: &'static [&'static str],
    /// Function to create an instance of the fixer
    pub create: fn() -> Box<dyn BuiltinFixer>,
}

inventory::collect!(BuiltinFixerRegistration);

/// Trait for implementing a builtin fixer
pub trait BuiltinFixer: Send + Sync {
    /// Name of the fixer
    fn name(&self) -> &'static str;

    /// Lintian tags this fixer addresses
    fn lintian_tags(&self) -> &'static [&'static str];

    /// Apply the fixer
    fn apply(
        &self,
        basedir: &std::path::Path,
        package: &str,
        current_version: &Version,
        preferences: &FixerPreferences,
    ) -> Result<FixerResult, FixerError>;
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

/// Get all registered builtin fixers
pub fn get_builtin_fixers() -> Vec<Box<dyn Fixer>> {
    inventory::iter::<BuiltinFixerRegistration>
        .into_iter()
        .map(|reg| {
            let builtin_fixer = (reg.create)();
            Box::new(BuiltinFixerWrapper::new(builtin_fixer)) as Box<dyn Fixer>
        })
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

        fn apply(
            &self,
            _basedir: &Path,
            _package: &str,
            _current_version: &Version,
            _preferences: &FixerPreferences,
        ) -> Result<FixerResult, FixerError> {
            Ok(FixerResult::builder("Mock fix applied")
                .fixed_issues(
                    self.tags
                        .iter()
                        .map(|s| LintianIssue::just_tag(s.to_string())),
                )
                .build())
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

        assert!(result.is_ok());
        let fixer_result = result.unwrap();
        assert_eq!(fixer_result.description, "Mock fix applied");
        assert_eq!(fixer_result.fixed_lintian_tags(), vec!["test-tag"]);
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

        fn apply(
            &self,
            _basedir: &Path,
            _package: &str,
            _current_version: &Version,
            _preferences: &FixerPreferences,
        ) -> Result<FixerResult, FixerError> {
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
}
