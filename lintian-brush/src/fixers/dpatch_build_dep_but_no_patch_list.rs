use crate::{Certainty, FixerError, FixerResult, LintianIssue};
use debian_control::lossless::Control;
use std::path::Path;
use std::str::FromStr;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    // Read debian/control
    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let control_content = std::fs::read_to_string(&control_path)?;
    let control = Control::from_str(&control_content)
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/control: {:?}", e)))?;

    let source = control
        .source()
        .ok_or_else(|| FixerError::Other("No source paragraph in debian/control".to_string()))?;

    // Check if dpatch is in Build-Depends-All
    let build_depends = source.build_depends().unwrap_or_default();
    let build_depends_indep = source.build_depends_indep().unwrap_or_default();
    let build_depends_arch = source.build_depends_arch().unwrap_or_default();

    let has_dpatch = build_depends.entries().any(|entry| {
        entry
            .relations()
            .any(|rel| rel.try_name().as_deref() == Some("dpatch"))
    }) || build_depends_indep.entries().any(|entry| {
        entry
            .relations()
            .any(|rel| rel.try_name().as_deref() == Some("dpatch"))
    }) || build_depends_arch.entries().any(|entry| {
        entry
            .relations()
            .any(|rel| rel.try_name().as_deref() == Some("dpatch"))
    });

    if !has_dpatch {
        return Err(FixerError::NoChanges);
    }

    // Check if debian/patches directory exists
    let patches_dir = base_path.join("debian/patches");
    if !patches_dir.exists() {
        // Create the debian/patches directory
        std::fs::create_dir_all(&patches_dir)?;
    }

    // Check if 00list exists (or any file starting with 00list)
    let has_list_file = patches_dir
        .read_dir()?
        .filter_map(|entry| entry.ok())
        .any(|entry| entry.file_name().to_string_lossy().starts_with("00list"));

    if has_list_file {
        return Err(FixerError::NoChanges);
    }

    let issue = LintianIssue::source_with_info("dpatch-build-dep-but-no-patch-list", vec![]);

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Create debian/patches/00list with a comment to prevent it from disappearing
    let list_file_path = patches_dir.join("00list");
    std::fs::write(
        &list_file_path,
        "# List patches to apply here\n# Empty file cannot be represented in Debian diff\n",
    )?;

    Ok(
        FixerResult::builder("Add missing debian/patches/00list file for dpatch.")
            .fixed_issues(vec![issue])
            .certainty(Certainty::Certain)
            .build(),
    )
}

declare_fixer! {
    name: "dpatch-build-dep-but-no-patch-list",
    tags: ["dpatch-build-dep-but-no-patch-list"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_creates_00list_when_dpatch_in_build_depends() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Test User <test@example.com>
Build-Depends: debhelper, dpatch

Package: test-package
Description: Test package
 Test description
"#;
        fs::write(debian_dir.join("control"), control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result
            .description
            .contains("Add missing debian/patches/00list file for dpatch"));

        // Check that the 00list file was created
        let list_file = temp_dir.path().join("debian/patches/00list");
        assert!(list_file.exists());

        // Check the content has comments
        let content = fs::read_to_string(&list_file).unwrap();
        assert!(content.contains("#"));
    }

    #[test]
    fn test_no_changes_when_no_dpatch() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Test User <test@example.com>
Build-Depends: debhelper

Package: test-package
Description: Test package
 Test description
"#;
        fs::write(debian_dir.join("control"), control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_00list_exists() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let patches_dir = debian_dir.join("patches");
        fs::create_dir_all(&patches_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Test User <test@example.com>
Build-Depends: debhelper, dpatch

Package: test-package
Description: Test package
 Test description
"#;
        fs::write(debian_dir.join("control"), control_content).unwrap();

        // Create existing 00list file
        fs::write(patches_dir.join("00list"), "# existing\n").unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_creates_patches_dir_if_missing() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Test User <test@example.com>
Build-Depends: debhelper, dpatch

Package: test-package
Description: Test package
 Test description
"#;
        fs::write(debian_dir.join("control"), control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(result.is_ok());

        // Check that the patches directory was created
        let patches_dir = temp_dir.path().join("debian/patches");
        assert!(patches_dir.exists());
        assert!(patches_dir.join("00list").exists());
    }

    #[test]
    fn test_dpatch_in_build_depends_indep() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Test User <test@example.com>
Build-Depends: debhelper
Build-Depends-Indep: dpatch

Package: test-package
Description: Test package
 Test description
"#;
        fs::write(debian_dir.join("control"), control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(result.is_ok());

        // Check that the 00list file was created
        let list_file = temp_dir.path().join("debian/patches/00list");
        assert!(list_file.exists());
    }
}
