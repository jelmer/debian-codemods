use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::debhelper::read_debhelper_compat_file;
use debian_control::lossless::Control;
use debversion::Version;
use std::path::Path;
use std::str::FromStr;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    // Read debian/compat to get the minimum debhelper version
    let compat_path = base_path.join("debian/compat");
    let minimum_version = match read_debhelper_compat_file(&compat_path)? {
        Some(v) => v,
        None => return Err(FixerError::NoChanges),
    };

    // Read and parse debian/control
    let control_path = base_path.join("debian/control");
    let control_content = std::fs::read_to_string(&control_path)?;
    let control = Control::from_str(&control_content)
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/control: {:?}", e)))?;

    let mut source = control
        .source()
        .ok_or_else(|| FixerError::Other("No source paragraph in debian/control".to_string()))?;

    // Get Build-Depends
    let mut build_depends = source.build_depends().unwrap_or_default();

    // Check if debhelper is already at the correct version
    let version_str = format!("{}~", minimum_version);
    let version = Version::from_str(&version_str)
        .map_err(|e| FixerError::Other(format!("Failed to parse version: {:?}", e)))?;

    let original_build_depends = build_depends.to_string();

    // Ensure minimum version for debhelper
    build_depends.ensure_minimum_version("debhelper", &version);

    // Check if anything changed
    if build_depends.to_string() == original_build_depends {
        return Err(FixerError::NoChanges);
    }

    source.set_build_depends(&build_depends);

    // Write back to file
    std::fs::write(&control_path, control.to_string())?;

    Ok(FixerResult::builder(format!(
        "Bump debhelper dependency to >= {}, since that's what is used in debian/compat.",
        minimum_version
    ))
    .fixed_tag("no-versioned-debhelper-prerequisite")
    .build())
}

declare_fixer! {
    name: "package-needs-versioned-debhelper-build-depends",
    tags: ["no-versioned-debhelper-prerequisite"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use debian_control::lossless::Control;
    use std::str::FromStr;

    #[test]
    fn test_ensure_minimum_version() {
        let input = r#"Source: blah
Maintainer: Joe Example <joe@example.com>
Build-Depends: debhelper (>= 9), pkg-config

Package: blah
Description: blah blah
"#;

        let control = Control::from_str(input).unwrap();
        let mut source = control.source().unwrap();
        let mut build_depends = source.build_depends().unwrap();

        let version = Version::from_str("12~").unwrap();
        build_depends.ensure_minimum_version("debhelper", &version);

        let output = build_depends.to_string();
        assert!(output.contains("debhelper"));
        assert!(output.contains(">= 12~"));
    }

    #[test]
    fn test_no_change_when_already_correct() {
        let input = r#"Source: blah
Maintainer: Joe Example <joe@example.com>
Build-Depends: debhelper (>= 12~), pkg-config

Package: blah
Description: blah blah
"#;

        let control = Control::from_str(input).unwrap();
        let mut source = control.source().unwrap();
        let mut build_depends = source.build_depends().unwrap();
        let original = build_depends.to_string();

        let version = Version::from_str("12~").unwrap();
        build_depends.ensure_minimum_version("debhelper", &version);

        assert_eq!(build_depends.to_string(), original);
    }
}
