//! Systemd specifier expansion support
//!
//! Systemd uses percent-escaped specifiers in unit files that are expanded
//! at runtime. This module provides functionality to expand these specifiers.

use std::collections::HashMap;

/// Context for expanding systemd specifiers
///
/// This contains the values that will be substituted when expanding
/// specifiers in unit file values.
#[derive(Debug, Clone, Default)]
pub struct SpecifierContext {
    values: HashMap<String, String>,
}

impl SpecifierContext {
    /// Create a new empty specifier context
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    /// Set a specifier value
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SpecifierContext;
    /// let mut ctx = SpecifierContext::new();
    /// ctx.set("i", "instance");
    /// ctx.set("u", "user");
    /// ```
    pub fn set(&mut self, specifier: &str, value: &str) {
        self.values.insert(specifier.to_string(), value.to_string());
    }

    /// Get a specifier value
    pub fn get(&self, specifier: &str) -> Option<&str> {
        self.values.get(specifier).map(|s| s.as_str())
    }

    /// Create a context with common system specifiers
    ///
    /// This sets up commonly used specifiers with their values:
    /// - `%n`: Unit name (without type suffix)
    /// - `%N`: Full unit name
    /// - `%p`: Prefix (for template units)
    /// - `%i`: Instance (for template units)
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SpecifierContext;
    /// let ctx = SpecifierContext::with_unit_name("foo@bar.service");
    /// assert_eq!(ctx.get("N"), Some("foo@bar.service"));
    /// assert_eq!(ctx.get("n"), Some("foo@bar"));
    /// assert_eq!(ctx.get("p"), Some("foo"));
    /// assert_eq!(ctx.get("i"), Some("bar"));
    /// ```
    pub fn with_unit_name(unit_name: &str) -> Self {
        let mut ctx = Self::new();

        // Full unit name
        ctx.set("N", unit_name);

        // Unit name without suffix
        let name_without_suffix = unit_name
            .rsplit_once('.')
            .map(|(name, _)| name)
            .unwrap_or(unit_name);
        ctx.set("n", name_without_suffix);

        // For template units (foo@instance.service)
        if let Some((prefix, instance_with_suffix)) = name_without_suffix.split_once('@') {
            ctx.set("p", prefix);
            ctx.set("i", instance_with_suffix);
        }

        ctx
    }

    /// Expand specifiers in a string
    ///
    /// This replaces all `%X` patterns with their corresponding values from the context.
    /// `%%` is replaced with a single `%`.
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SpecifierContext;
    /// let mut ctx = SpecifierContext::new();
    /// ctx.set("i", "myinstance");
    /// ctx.set("u", "myuser");
    ///
    /// let result = ctx.expand("/var/lib/%i/data/%u");
    /// assert_eq!(result, "/var/lib/myinstance/data/myuser");
    /// ```
    pub fn expand(&self, input: &str) -> String {
        let mut result = String::new();
        let mut chars = input.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '%' {
                if let Some(&next) = chars.peek() {
                    chars.next(); // consume the peeked character
                    if next == '%' {
                        // %% -> %
                        result.push('%');
                    } else {
                        // %X -> lookup
                        let specifier = next.to_string();
                        if let Some(value) = self.get(&specifier) {
                            result.push_str(value);
                        } else {
                            // Unknown specifier, keep as-is
                            result.push('%');
                            result.push(next);
                        }
                    }
                } else {
                    // % at end of string
                    result.push('%');
                }
            } else {
                result.push(ch);
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_expansion() {
        let mut ctx = SpecifierContext::new();
        ctx.set("i", "instance");
        ctx.set("u", "user");

        assert_eq!(ctx.expand("Hello %i"), "Hello instance");
        assert_eq!(ctx.expand("%u@%i"), "user@instance");
        assert_eq!(ctx.expand("/home/%u/%i"), "/home/user/instance");
    }

    #[test]
    fn test_percent_escape() {
        let ctx = SpecifierContext::new();
        assert_eq!(ctx.expand("100%% complete"), "100% complete");
        assert_eq!(ctx.expand("%%u"), "%u");
    }

    #[test]
    fn test_unknown_specifier() {
        let ctx = SpecifierContext::new();
        // Unknown specifiers are kept as-is
        assert_eq!(ctx.expand("%x"), "%x");
        assert_eq!(ctx.expand("test %z end"), "test %z end");
    }

    #[test]
    fn test_percent_at_end() {
        let ctx = SpecifierContext::new();
        assert_eq!(ctx.expand("test%"), "test%");
    }

    #[test]
    fn test_with_unit_name_simple() {
        let ctx = SpecifierContext::with_unit_name("foo.service");
        assert_eq!(ctx.get("N"), Some("foo.service"));
        assert_eq!(ctx.get("n"), Some("foo"));
        assert_eq!(ctx.get("p"), None);
        assert_eq!(ctx.get("i"), None);
    }

    #[test]
    fn test_with_unit_name_template() {
        let ctx = SpecifierContext::with_unit_name("foo@bar.service");
        assert_eq!(ctx.get("N"), Some("foo@bar.service"));
        assert_eq!(ctx.get("n"), Some("foo@bar"));
        assert_eq!(ctx.get("p"), Some("foo"));
        assert_eq!(ctx.get("i"), Some("bar"));

        assert_eq!(ctx.expand("Unit %N"), "Unit foo@bar.service");
        assert_eq!(ctx.expand("Prefix %p"), "Prefix foo");
        assert_eq!(ctx.expand("Instance %i"), "Instance bar");
    }

    #[test]
    fn test_with_unit_name_complex_instance() {
        let ctx = SpecifierContext::with_unit_name("getty@tty1.service");
        assert_eq!(ctx.get("p"), Some("getty"));
        assert_eq!(ctx.get("i"), Some("tty1"));
        assert_eq!(ctx.expand("/dev/%i"), "/dev/tty1");
    }

    #[test]
    fn test_multiple_specifiers() {
        let mut ctx = SpecifierContext::new();
        ctx.set("i", "inst");
        ctx.set("u", "usr");
        ctx.set("h", "/home/usr");

        assert_eq!(
            ctx.expand("%h/.config/%i/data"),
            "/home/usr/.config/inst/data"
        );
    }

    #[test]
    fn test_no_specifiers() {
        let ctx = SpecifierContext::new();
        assert_eq!(ctx.expand("plain text"), "plain text");
        assert_eq!(ctx.expand("/etc/config"), "/etc/config");
    }
}
