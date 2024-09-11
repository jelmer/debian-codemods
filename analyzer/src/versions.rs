/// Make an upstream version string suitable for Debian.
///
/// # Arguments
/// * `version` - original upstream version string
///
/// # Returns
/// mangled version string for use in Debian versions
pub fn debianize_upstream_version(version: &str) -> String {
    let mut version = version.to_string();
    if version.chars().filter(|c| *c == '_').count() == 1
        && version.chars().filter(|c| *c == '.').count() > 1
    {
        // This is a style commonly used for perl packages.
        // Most debian packages seem to just drop the underscore.
        // See
        // http://blogs.perl.org/users/grinnz/2018/04/a-guide-to-versions-in-perl.html
        version = version.replace('_', "");
    }
    if version.contains('_') && !version.contains('.') {
        version = version.replace('_', ".");
    }
    version = version.replace("-rc", "~rc");
    version = version.replace("-beta", "~beta");
    version = version.replace("-alpha", "~alpha");
    if let Some((_, a, b, c, d)) =
        lazy_regex::regex_captures!(r"(.*)\.([0-9])(a|b|rc|alpha|beta)([0-9]*)", &version)
    {
        version = format!("{}.{}~{}{}", a, b, c, d);
    }
    version
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
