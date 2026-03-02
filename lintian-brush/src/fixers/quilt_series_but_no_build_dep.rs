use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::relations::ensure_some_version;
use debian_control::lossless::Control;
use std::path::Path;
use std::str::FromStr;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    // Check if this is a debcargo package
    if base_path.join("debian/debcargo.toml").exists() {
        return Err(FixerError::NoChanges);
    }

    // Read the source format
    let source_format_path = base_path.join("debian/source/format");
    let format = if source_format_path.exists() {
        std::fs::read_to_string(&source_format_path)?
            .trim()
            .to_string()
    } else {
        String::new()
    };

    // Skip if using 3.0 (quilt) format
    if format == "3.0 (quilt)" {
        return Err(FixerError::NoChanges);
    }

    // Check if debian/patches/series exists
    if !base_path.join("debian/patches/series").exists() {
        return Err(FixerError::NoChanges);
    }

    // Update the control file
    let control_path = base_path.join("debian/control");
    let control_content = std::fs::read_to_string(&control_path)?;
    let control = Control::from_str(&control_content)
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/control: {:?}", e)))?;

    let mut source = control
        .source()
        .ok_or_else(|| FixerError::Other("No source paragraph in debian/control".to_string()))?;

    let original_build_depends = source.build_depends().unwrap_or_default();
    let original_str = original_build_depends.to_string();

    // Parse into a mutable copy
    let mut new_build_depends = original_build_depends;

    ensure_some_version(&mut new_build_depends, "quilt");

    // Check if anything changed
    if new_build_depends.to_string() == original_str {
        return Err(FixerError::NoChanges);
    }

    let issue = LintianIssue::source_with_info(
        "quilt-series-but-no-build-dep",
        vec!["[debian/patches/series]".to_string()],
    );

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    source.set_build_depends(&new_build_depends);

    // Write back to file
    std::fs::write(&control_path, control.to_string())?;

    Ok(FixerResult::builder("Add missing dependency on quilt.")
        .fixed_issues(vec![issue])
        .build())
}

declare_fixer! {
    name: "quilt-series-but-no-build-dep",
    tags: ["quilt-series-but-no-build-dep"],
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
    fn test_ensure_some_version_adds_quilt() {
        let input = r#"Source: blah
Maintainer: Joe Example <joe@example.com>
Build-Depends: debhelper

Package: blah
Description: blah blah
 Blah blah
"#;

        let control = Control::from_str(input).unwrap();
        let mut source = control.source().unwrap();
        let mut build_depends = source.build_depends().unwrap_or_default();

        ensure_some_version(&mut build_depends, "quilt");
        source.set_build_depends(&build_depends);

        let output = control.to_string();
        assert!(output.contains("Build-Depends: debhelper, quilt"));
    }

    #[test]
    fn test_ensure_some_version_skips_if_present() {
        let input = r#"Source: blah
Maintainer: Joe Example <joe@example.com>
Build-Depends: debhelper, quilt

Package: blah
Description: blah blah
 Blah blah
"#;

        let control = Control::from_str(input).unwrap();
        let source = control.source().unwrap();
        let original_build_depends = source.build_depends().unwrap_or_default();
        let original_str = original_build_depends.to_string();
        let mut new_build_depends = original_build_depends;

        ensure_some_version(&mut new_build_depends, "quilt");

        assert_eq!(new_build_depends.to_string(), original_str);
    }
}
