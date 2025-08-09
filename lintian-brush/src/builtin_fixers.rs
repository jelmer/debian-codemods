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
}