//! Debian version handling utilities.

/// Make an upstream version string suitable for Debian.
///
/// # Arguments
/// * `version` - original upstream version string
///
/// # Returns
/// mangled version string for use in Debian versions
pub fn debianize_upstream_version(version: &str) -> String {
    use std::borrow::Cow;

    let mut version = Cow::Borrowed(version);

    // Count underscores and dots to determine if we need to modify
    let underscore_count = version.chars().filter(|c| *c == '_').count();
    let dot_count = version.chars().filter(|c| *c == '.').count();

    if underscore_count == 1 && dot_count > 1 {
        // This is a style commonly used for perl packages.
        // Most debian packages seem to just drop the underscore.
        // See
        // http://blogs.perl.org/users/grinnz/2018/04/a-guide-to-versions-in-perl.html
        version = Cow::Owned(version.replace('_', ""));
    } else if underscore_count > 0 && dot_count == 0 {
        version = Cow::Owned(version.replace('_', "."));
    }

    // Replace pre-release indicators
    if version.contains("-rc") || version.contains("-beta") || version.contains("-alpha") {
        let mut owned = version.into_owned();
        owned = owned.replace("-rc", "~rc");
        owned = owned.replace("-beta", "~beta");
        owned = owned.replace("-alpha", "~alpha");
        version = Cow::Owned(owned);
    }

    if let Some((_, a, b, c, d)) =
        lazy_regex::regex_captures!(r"(.*)\.([0-9])(a|b|rc|alpha|beta)([0-9]*)", &version)
    {
        return format!("{}.{}~{}{}", a, b, c, d);
    }

    version.into_owned()
}

/// Check whether an upstream version string matches a upstream release.
///
/// This will e.g. strip git and dfsg suffixes before comparing.
///
/// # Arguments
/// * `upstream_version` - Upstream version string
/// * `release_version` - Release to check for
pub fn matches_release(upstream_version: &str, release_version: &str) -> bool {
    let release_version = release_version.to_lowercase();
    let upstream_version = upstream_version.to_lowercase();
    if upstream_version == release_version {
        return true;
    }
    if let Some((_, base, _)) =
        lazy_regex::regex_captures!(r"(.*)[~+-](ds|dfsg|git|bzr|svn|hg).*", &upstream_version)
    {
        if base == release_version {
            return true;
        }
    }
    if let Some((_, base)) = lazy_regex::regex_captures!(r"(.*)[~+-].*", &upstream_version) {
        if base == release_version {
            return true;
        }
    }
    if let Some((_, lead)) = lazy_regex::regex_captures!(".*~([0-9.]+)$", &upstream_version) {
        if lead == release_version {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_debianize_upstream_version() {
        assert_eq!(debianize_upstream_version("1.0"), "1.0");
        assert_eq!(debianize_upstream_version("1.0-beta1"), "1.0~beta1");
        assert_eq!(debianize_upstream_version("1.0-rc1"), "1.0~rc1");
        assert_eq!(debianize_upstream_version("1.0a1"), "1.0~a1");
    }

    #[test]
    fn test_matches_release() {
        assert!(matches_release("1.0", "1.0"));
        assert!(matches_release("1.0+ds1", "1.0"));
        assert!(matches_release("1.14.3+dfsg+~0.15.3", "0.15.3"));
        assert!(!matches_release("1.0", "1.1"));
        assert!(!matches_release("1.0+ds1", "1.1"));
    }
}
