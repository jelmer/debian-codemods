use super::*;

/// Registration information for a builtin fixer
pub struct BuiltinFixerRegistration {
    pub name: &'static str,
    pub lintian_tags: &'static [&'static str],
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
        self.fixer
            .apply(basedir, package, current_version, preferences)
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
                .fixed_tags(self.tags.iter().map(|s| s.to_string()))
                .build())
        }
    }

    #[test]
    fn test_get_builtin_fixers() {
        let fixers = get_builtin_fixers();
        // Check that we have at least one fixer (the CRLF fixer)
        assert!(!fixers.is_empty(), "No builtin fixers found");

        // Check that the CRLF fixer is registered
        let crlf_fixer = fixers
            .iter()
            .find(|f| f.name() == "control-file-with-CRLF-EOLs");
        assert!(crlf_fixer.is_some(), "CRLF fixer not found");
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
}
