//! Lintian overrides parsing and manipulation.
//!
//! This module re-exports everything from the `lintian-overrides` crate and adds
//! an extension trait for matching override lines against [`LintianIssue`](crate::LintianIssue)s.

pub use lintian_overrides::*;

/// Extension trait for matching override lines against LintianIssue
pub trait OverrideLineMatch {
    /// Check if this override matches a LintianIssue
    fn matches_issue(&self, issue: &crate::LintianIssue) -> bool;
}

impl OverrideLineMatch for OverrideLine {
    fn matches_issue(&self, issue: &crate::LintianIssue) -> bool {
        self.matches(
            issue.tag.as_deref(),
            issue.package.as_deref(),
            issue
                .package_type
                .as_ref()
                .map(|t| t.to_string())
                .as_deref(),
            issue.info.as_deref(),
        )
    }
}

/// Check if a tag has a defined info fixer
/// Returns true if fix_override_info has a transformation defined for this tag
pub fn has_info_fixer(tag: &str) -> bool {
    // Test if fix_override_info would transform the info
    // If the result is different from input (with a dummy info string), we have a fixer
    let test_info = "test (line 1)";
    let result = fix_override_info(tag, test_info);
    result != test_info
}

/// Fix override info format by applying tag-specific transformations
/// Converts old format like "file (line 123)" to new format like "[file:123]"
pub fn fix_override_info(tag: &str, info: &str) -> String {
    use lazy_static::lazy_static;
    use regex::Regex;

    lazy_static! {
        // Common regex patterns - note: Rust regex doesn't support lookahead, so we match anything not [ or space
        static ref PATH_MATCH: &'static str = r"(?P<path>[^\[\s]+)";
        static ref LINENO_MATCH: &'static str = r"(?P<lineno>\d+|\*)";

        // Pure file:lineno transformations
        static ref PURE_FLN_RE: Regex = Regex::new(&format!(r"^{} \(line {}\)$", *PATH_MATCH, *LINENO_MATCH)).unwrap();
        static ref PURE_FLN_WILDCARD_RE: Regex = Regex::new(&format!(r"^{} \(line {}\)$", *PATH_MATCH, *LINENO_MATCH)).unwrap();
        static ref PURE_FN_RE: Regex = Regex::new(&format!(r"^{}$", *PATH_MATCH)).unwrap();

        // Debian rules specific
        static ref RULES_LINENO_RE: Regex = Regex::new(&format!(r"(.*) \(line {}\)", *LINENO_MATCH)).unwrap();

        // Debian source options
        static ref SOURCE_OPTIONS_RE: Regex = Regex::new(&format!(r"(.*) \(line {}\)", *LINENO_MATCH)).unwrap();

        // Copyright file patterns
        static ref COPYRIGHT_LINE_RE: Regex = Regex::new(&format!(r"^debian/copyright (.+) \(line {}\)", *LINENO_MATCH)).unwrap();
        static ref COPYRIGHT_WILDCARD_RE: Regex = Regex::new(r"^debian/copyright (.+) \*").unwrap();
        static ref COPYRIGHT_STAR_RE: Regex = Regex::new(r"^debian/copyright \*").unwrap();
        static ref COPYRIGHT_SIMPLE_RE: Regex = Regex::new(r"^([^/ ]+) \*").unwrap();

        // Permission-related
        static ref NON_STANDARD_PERM_RE: Regex = Regex::new(&format!(r"^{} ([0-9]+) != ([0-9]+)", *PATH_MATCH)).unwrap();
        static ref EXECUTABLE_PERM_RE: Regex = Regex::new(&format!(r"^{} ([0-9]+)", *PATH_MATCH)).unwrap();
        static ref SETUID_RE: Regex = Regex::new(&format!(r"^{} (?P<mode>[0-9]+) (.+/.+)", *PATH_MATCH)).unwrap();

        // Man page errors
        static ref MANPAGE_RE: Regex = Regex::new(&format!(r"^{} ([^\[]*)", *PATH_MATCH)).unwrap();
        static ref GROFF_RE: Regex = Regex::new(&format!(r"^{} ([0-9]+): (.+)$", *PATH_MATCH)).unwrap();

        // Version substvar
        static ref VERSION_SUBSTVAR_RE: Regex = Regex::new(&format!(r"([^ ]+) \(line {}\) (.*)", *LINENO_MATCH)).unwrap();
    }

    match tag {
        "autotools-pkg-config-macro-not-cross-compilation-safe" => {
            if let Some(caps) = PURE_FLN_WILDCARD_RE.captures(info) {
                return format!("* [{}:{}]", &caps["path"], &caps["lineno"]);
            }
        }
        "debian-rules-parses-dpkg-parsechangelog"
        | "global-files-wildcard-not-first-paragraph-in-dep5-copyright" => {
            if let Some(caps) = PURE_FLN_RE.captures(info) {
                return format!("[{}:{}]", &caps["path"], &caps["lineno"]);
            }
        }
        "debian-rules-should-not-use-custom-compression-settings" => {
            if let Some(caps) = RULES_LINENO_RE.captures(info) {
                return format!("{} [debian/rules:{}]", &caps[1], &caps["lineno"]);
            }
        }
        "debian-source-options-has-custom-compression-settings" => {
            if let Some(caps) = SOURCE_OPTIONS_RE.captures(info) {
                return format!("{} [debian/source/options:{}]", &caps[1], &caps["lineno"]);
            }
        }
        "missing-license-paragraph-in-dep5-copyright"
        | "missing-license-text-in-dep5-copyright" => {
            // Apply multiple copyright transformations in order
            let mut result = info.to_string();
            if let Some(caps) = COPYRIGHT_LINE_RE.captures(&result) {
                result = format!("{} [debian/copyright:{}]", &caps[1], &caps["lineno"]);
            } else if let Some(caps) = COPYRIGHT_WILDCARD_RE.captures(&result) {
                result = format!("{} [debian/copyright:*]", &caps[1]);
            } else if COPYRIGHT_STAR_RE.is_match(&result) {
                result = "* [debian/copyright:*]".to_string();
            } else if let Some(caps) = COPYRIGHT_SIMPLE_RE.captures(&result) {
                result = format!("{} [debian/copyright:*]", &caps[1]);
            }
            return result;
        }
        "unused-license-paragraph-in-dep5-copyright" => {
            let re = Regex::new(&format!(r"([^ ]+) (.*) \(line {}\)", *LINENO_MATCH)).unwrap();
            if let Some(caps) = re.captures(info) {
                return format!("{} [{}:{}]", &caps[2], &caps[1], &caps["lineno"]);
            }
        }
        "license-problem-undefined-license" | "incomplete-creative-commons-license" => {
            if let Some(caps) = RULES_LINENO_RE.captures(info) {
                return format!("{} [debian/copyright:{}]", &caps[1], &caps["lineno"]);
            }
        }
        "debhelper-tools-from-autotools-dev-are-deprecated"
        | "debian-rules-sets-dpkg-architecture-variable"
        | "override_dh_auto_test-does-not-check-DEB_BUILD_OPTIONS"
        | "dh-quilt-addon-but-quilt-source-format" => {
            if let Some(caps) = RULES_LINENO_RE.captures(info) {
                return format!("{} [debian/rules:{}]", &caps[1], &caps["lineno"]);
            }
        }
        "uses-deprecated-adttmp" => {
            let re = Regex::new(&format!(r"([^ ]+) \(line {}\)", *LINENO_MATCH)).unwrap();
            if let Some(caps) = re.captures(info) {
                return format!("[{}:{}]", &caps[1], &caps["lineno"]);
            }
        }
        "debian-watch-uses-insecure-uri" => {
            if let Some(caps) = RULES_LINENO_RE.captures(info) {
                return format!("{} [debian/watch:{}]", &caps[1], &caps["lineno"]);
            }
        }
        "uses-dpkg-database-directly"
        | "package-contains-documentation-outside-usr-share-doc"
        | "library-not-linked-against-libc"
        | "executable-in-usr-lib"
        | "executable-not-elf-or-script"
        | "image-file-in-usr-lib"
        | "extra-license-file"
        | "script-not-executable"
        | "shell-script-fails-syntax-check"
        | "source-contains-prebuilt-java-object"
        | "source-contains-prebuilt-windows-binary"
        | "source-contains-prebuilt-doxygen-documentation"
        | "source-contains-prebuilt-wasm-binary"
        | "source-contains-prebuilt-binary"
        | "hardening-no-fortify-functions" => {
            if let Some(caps) = PURE_FN_RE.captures(info) {
                return format!("[{}]", &caps["path"]);
            }
        }
        "non-standard-dir-perm" | "non-standard-file-perm" => {
            if let Some(caps) = NON_STANDARD_PERM_RE.captures(info) {
                return format!("{} != {} [{}]", &caps[2], &caps[3], &caps["path"]);
            }
        }
        "executable-is-not-world-readable" => {
            if let Some(caps) = EXECUTABLE_PERM_RE.captures(info) {
                return format!("{} [{}]", &caps[2], &caps["path"]);
            }
        }
        "setuid-binary" | "elevated-privileges" => {
            if let Some(caps) = SETUID_RE.captures(info) {
                return format!("{} {} [{}]", &caps["mode"], &caps[3], &caps["path"]);
            }
        }
        "manpage-has-errors-from-man" => {
            if let Some(caps) = MANPAGE_RE.captures(info) {
                return format!("{} [{}]", &caps[2], &caps["path"]);
            }
        }
        "groff-message" => {
            if let Some(caps) = GROFF_RE.captures(info) {
                return format!("{}: {} [{}:*]", &caps[2], &caps[3], &caps["path"]);
            }
        }
        "source-contains-prebuilt-javascript-object" => {
            if let Some(caps) = PURE_FN_RE.captures(info) {
                return format!("[{}]", &caps["path"]);
            }
            let line_len_re = Regex::new(r"^(?P<path>[^\[ ].+) line length is .*").unwrap();
            if let Some(caps) = line_len_re.captures(info) {
                return format!("[{}]", &caps["path"]);
            }
        }
        "version-substvar-for-external-package" => {
            if let Some(caps) = VERSION_SUBSTVAR_RE.captures(info) {
                return format!(
                    "{} {} [debian/control:{}]",
                    &caps[1], &caps[3], &caps["lineno"]
                );
            }
        }
        _ => {}
    }

    // No transformation matched, return original info
    info.to_string()
}
