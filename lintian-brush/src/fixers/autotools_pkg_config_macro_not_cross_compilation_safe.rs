use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::relations::ensure_some_version;
use debian_control::lossless::Control;
use regex::bytes::Regex;
use std::fs;
use std::path::Path;
use std::str::FromStr;

fn update_configure_ac(path: &Path) -> Result<(bool, String, usize), std::io::Error> {
    if !path.exists() {
        return Ok((false, String::new(), 0));
    }

    let content = fs::read(path)?;
    let mut changed = false;
    let mut resolution = String::new();
    let mut line_number = 0;

    // Regex to match AC_PATH_PROG with pkg-config
    // Pattern: \s*AC_PATH_PROG\s*\(\s*(\[)?(?P<variable>[A-Z_]+)(\])?\s*,\s*(\[)?pkg-config(\])?\s*(,\s*(\[)?(?P<default>.*)(\])?\s*)?\)
    let re = Regex::new(
        r"(?m)^\s*AC_PATH_PROG\s*\(\s*(\[)?(?P<variable>[A-Z_]+)(\])?\s*,\s*(\[)?pkg-config(\])?\s*(,\s*(\[)?(?P<default>.*)(\])?\s*)?\)\n"
    ).unwrap();

    let mut new_content = Vec::new();
    let mut last_end = 0;

    for caps in re.captures_iter(&content) {
        let full_match = caps.get(0).unwrap();
        let variable = caps.name("variable").unwrap().as_bytes();
        let default = caps.name("default").map(|m| m.as_bytes());

        // Calculate line number (count newlines before this match)
        if line_number == 0 {
            line_number = content[..full_match.start()]
                .iter()
                .filter(|&&b| b == b'\n')
                .count()
                + 1;
        }

        // Add content before this match
        new_content.extend_from_slice(&content[last_end..full_match.start()]);

        // Determine the replacement
        if variable == b"PKG_CONFIG" && default.is_none() {
            new_content.extend_from_slice(b"PKG_PROG_PKG_CONFIG\n");
            resolution =
                "This patch changes it to use PKG_PROG_PKG_CONFIG macro from pkg.m4.".to_string();
        } else {
            // Replace AC_PATH_PROG with AC_PATH_TOOL
            let original = full_match.as_bytes();
            let replaced = original
                .windows(b"AC_PATH_PROG".len())
                .enumerate()
                .find(|(_, w)| *w == b"AC_PATH_PROG")
                .map(|(i, _)| {
                    let mut result = original[..i].to_vec();
                    result.extend_from_slice(b"AC_PATH_TOOL");
                    result.extend_from_slice(&original[i + b"AC_PATH_PROG".len()..]);
                    result
                })
                .unwrap_or_else(|| original.to_vec());

            new_content.extend_from_slice(&replaced);
            resolution = "This patch changes it to use AC_PATH_TOOL.".to_string();
        }

        changed = true;
        last_end = full_match.end();
    }

    if changed {
        // Add remaining content
        new_content.extend_from_slice(&content[last_end..]);
        fs::write(path, new_content)?;
        Ok((true, resolution, line_number))
    } else {
        Ok((false, String::new(), 0))
    }
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let mut changed = false;
    let mut resolution = String::new();
    let mut configure_file = String::new();
    let mut line_number = 0;

    // Try both configure.ac and configure.in
    for name in &["configure.ac", "configure.in"] {
        let path = base_path.join(name);
        match update_configure_ac(&path) {
            Ok((true, res, line)) => {
                changed = true;
                resolution = res;
                configure_file = name.to_string();
                line_number = line;
            }
            Ok((false, _, _)) => {}
            Err(e) => return Err(FixerError::from(e)),
        }
    }

    if !changed {
        return Err(FixerError::NoChanges);
    }

    // Create issue and check if we should fix it
    let issue = LintianIssue::source_with_info(
        "autotools-pkg-config-macro-not-cross-compilation-safe",
        vec![format!("AC_PATH_PROG [{}:{}]", configure_file, line_number)],
    );
    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Add pkg-config to Build-Depends if we used PKG_PROG_PKG_CONFIG
    if resolution.contains("PKG_PROG_PKG_CONFIG") {
        let control_path = base_path.join("debian/control");
        if control_path.exists() {
            let control_content = fs::read_to_string(&control_path)?;
            let control = Control::from_str(&control_content).map_err(|e| {
                FixerError::Other(format!("Failed to parse debian/control: {:?}", e))
            })?;

            let mut source = control.source().ok_or_else(|| {
                FixerError::Other("No source paragraph in debian/control".to_string())
            })?;

            let original_build_depends = source.build_depends().unwrap_or_default();
            let original_str = original_build_depends.to_string();

            let mut new_build_depends = original_build_depends;
            ensure_some_version(&mut new_build_depends, "pkg-config");

            // Only write if changed
            if new_build_depends.to_string() != original_str {
                source.set_build_depends(&new_build_depends);
                fs::write(&control_path, control.to_string())?;
            }
        }
    }

    Ok(FixerResult::builder(format!(
        "Use cross-build compatible macro for finding pkg-config.\n\n\
        The package uses AC_PATH_PROG to discover the location of pkg-config(1). This\n\
        macro fails to select the correct version to support cross-compilation.\n\n\
        {}\n\n\
        Refer to https://bugs.debian.org/884798 for details.\n",
        resolution
    ))
    .fixed_issues(vec![issue])
    .patch_name("ac-path-pkgconfig")
    .build())
}

declare_fixer! {
    name: "autotools-pkg-config-macro-not-cross-compilation-safe",
    tags: ["autotools-pkg-config-macro-not-cross-compilation-safe"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_replace_ac_path_prog_with_pkg_prog_pkg_config() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let configure_ac = base_path.join("configure.ac");
        fs::write(
            &configure_ac,
            b"AC_INIT([test], [1.0])\n\
              AC_PATH_PROG([PKG_CONFIG], [pkg-config])\n\
              AC_OUTPUT\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let content = fs::read_to_string(&configure_ac).unwrap();
        assert!(content.contains("PKG_PROG_PKG_CONFIG"));
        assert!(!content.contains("AC_PATH_PROG([PKG_CONFIG], [pkg-config])"));

        let result = result.unwrap();
        assert!(result
            .description
            .contains("PKG_PROG_PKG_CONFIG macro from pkg.m4"));
    }

    #[test]
    fn test_replace_ac_path_prog_with_ac_path_tool() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let configure_ac = base_path.join("configure.ac");
        fs::write(
            &configure_ac,
            b"AC_INIT([test], [1.0])\n\
              AC_PATH_PROG([PKGCONFIG], [pkg-config])\n\
              AC_OUTPUT\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let content = fs::read_to_string(&configure_ac).unwrap();
        assert!(content.contains("AC_PATH_TOOL([PKGCONFIG], [pkg-config])"));
        assert!(!content.contains("AC_PATH_PROG([PKGCONFIG], [pkg-config])"));

        let result = result.unwrap();
        assert!(result.description.contains("AC_PATH_TOOL"));
    }

    #[test]
    fn test_replace_ac_path_prog_with_default() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let configure_ac = base_path.join("configure.ac");
        fs::write(
            &configure_ac,
            b"AC_INIT([test], [1.0])\n\
              AC_PATH_PROG([PKG_CONFIG], [pkg-config], [no])\n\
              AC_OUTPUT\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let content = fs::read_to_string(&configure_ac).unwrap();
        assert!(content.contains("AC_PATH_TOOL([PKG_CONFIG], [pkg-config], [no])"));
        assert!(!content.contains("AC_PATH_PROG"));
    }

    #[test]
    fn test_no_changes_when_no_ac_path_prog() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let configure_ac = base_path.join("configure.ac");
        fs::write(
            &configure_ac,
            b"AC_INIT([test], [1.0])\n\
              PKG_PROG_PKG_CONFIG\n\
              AC_OUTPUT\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_no_configure_ac() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_updates_build_depends_for_pkg_prog_pkg_config() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let configure_ac = base_path.join("configure.ac");
        fs::write(
            &configure_ac,
            b"AC_INIT([test], [1.0])\n\
              AC_PATH_PROG([PKG_CONFIG], [pkg-config])\n\
              AC_OUTPUT\n",
        )
        .unwrap();

        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            b"Source: test\nMaintainer: Test <test@example.com>\nBuild-Depends: debhelper\n\nPackage: test\nDescription: Test package\n Test\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let control_content = fs::read_to_string(&control_path).unwrap();
        assert!(control_content.contains("pkg-config"));
    }
}
