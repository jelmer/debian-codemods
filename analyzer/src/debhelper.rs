//! Debhelper utilities.
use std::path::Path;

/// Parse the debhelper compat level from a string.
fn parse_debhelper_compat(s: &str) -> Option<u8> {
    s.split_once('#').map_or(s, |s| s.0).trim().parse().ok()
}

/// Read a debian/compat file.
///
/// # Arguments
/// * `path` - The path to the debian/compat file.
pub fn read_debhelper_compat_file(path: &Path) -> Result<Option<u8>, std::io::Error> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(parse_debhelper_compat(&content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Retrieve the debhelper compat level from a debian/control file.
///
/// # Arguments
/// * `control` - The debian/control file.
///
/// # Returns
/// The debhelper compat level.
pub fn get_debhelper_compat_level_from_control(control: &debian_control::Control) -> Option<u8> {
    let source = control.source()?;

    let build_depends = source.build_depends()?;

    let rels = build_depends
        .entries()
        .flat_map(|entry| entry.relations().collect::<Vec<_>>())
        .find(|r| r.name() == "debhelper-compat");

    rels.and_then(|r| r.version().and_then(|v| v.1.to_string().parse().ok()))
}

/// Retrieve the debhelper compat level from a debian/compat file or debian/control file.
///
/// # Arguments
/// * `path` - The path to the debian/ directory.
///
/// # Returns
/// The debhelper compat level.
pub fn get_debhelper_compat_level(path: &Path) -> Result<Option<u8>, std::io::Error> {
    match read_debhelper_compat_file(&path.join("debian/compat")) {
        Ok(Some(level)) => {
            return Ok(Some(level));
        }
        Err(e) => {
            return Err(e);
        }
        Ok(None) => {}
    }

    let p = path.join("debian/control");

    match std::fs::File::open(p) {
        Ok(f) => {
            let control = debian_control::Control::read_relaxed(f).unwrap().0;
            Ok(get_debhelper_compat_level_from_control(&control))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Retrieve the maximum supported debhelper compat version fior a release.
///
/// # Arguments
/// * `compat_release` - A release name (Debian or Ubuntu, currently)
///
/// # Returns
/// The debhelper compat version
pub fn maximum_debhelper_compat_version(compat_release: &str) -> u8 {
    crate::release_info::debhelper_versions
        .get(compat_release)
        .map(|v| {
            v.upstream_version
                .split('.')
                .next()
                .unwrap()
                .parse()
                .unwrap()
        })
        .unwrap_or_else(|| lowest_non_deprecated_compat_level())
}

/// Ask dh_assistant for the supported compat levels.
///
/// Cache the result.
fn get_lintian_compat_levels() -> &'static SupportedCompatLevels {
    lazy_static::lazy_static! {
        static ref LINTIAN_COMPAT_LEVELS: SupportedCompatLevels = {
            // TODO(jelmer): ideally we should be getting these numbers from the compat-release
            // dh_assistant, rather than what's on the system
            let output = std::process::Command::new("dh_assistant")
                .arg("supported-compat-levels")
                .output()
                .expect("failed to run dh_assistant")
                .stdout;
            serde_json::from_slice(&output).expect("failed to parse dh_assistant output")
        };
    };
    &LINTIAN_COMPAT_LEVELS
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct SupportedCompatLevels {
    #[serde(rename = "HIGHEST_STABLE_COMPAT_LEVEL")]
    highest_stable_compat_level: u8,
    #[serde(rename = "LOWEST_NON_DEPRECATED_COMPAT_LEVEL")]
    lowest_non_deprecated_compat_level: u8,
    #[serde(rename = "LOWEST_VIRTUAL_DEBHELPER_COMPAT_LEVEL")]
    lowest_virtual_debhelper_compat_level: u8,
    #[serde(rename = "MAX_COMPAT_LEVEL")]
    max_compat_level: u8,
    #[serde(rename = "MIN_COMPAT_LEVEL")]
    min_compat_level: u8,
    #[serde(rename = "MIN_COMPAT_LEVEL_NOT_SCHEDULED_FOR_REMOVAL")]
    min_compat_level_not_scheduled_for_removal: u8,
}

/// Find the lowest non-deprecated debhelper compat level.
pub fn lowest_non_deprecated_compat_level() -> u8 {
    get_lintian_compat_levels().lowest_non_deprecated_compat_level
}

/// Find the highest stable debhelper compat level.
pub fn highest_stable_compat_level() -> u8 {
    get_lintian_compat_levels().highest_stable_compat_level
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_debhelper_compat() {
        assert_eq!(super::parse_debhelper_compat("9"), Some(9));
        assert_eq!(super::parse_debhelper_compat("9 # comment"), Some(9));
        assert_eq!(
            super::parse_debhelper_compat("9 # comment # comment"),
            Some(9)
        );
        assert_eq!(super::parse_debhelper_compat(""), None);
        assert_eq!(super::parse_debhelper_compat(" # comment"), None);
    }

    #[test]
    fn test_get_debhelper_compat_level_from_control() {
        let text = "Source: foo
Build-Depends: debhelper-compat (= 9)

Package: foo
Architecture: any
";

        let control = debian_control::Control::read_relaxed(&mut text.as_bytes())
            .unwrap()
            .0;

        assert_eq!(
            super::get_debhelper_compat_level_from_control(&control),
            Some(9)
        );
    }
}
