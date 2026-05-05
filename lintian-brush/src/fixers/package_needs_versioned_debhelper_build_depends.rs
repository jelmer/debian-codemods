use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_analyzer::debhelper::read_debhelper_compat_file;
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let Some(minimum_version) = read_debhelper_compat_file(&base_path.join("debian/compat"))?
    else {
        return Ok(Vec::new());
    };

    let control_rel = PathBuf::from("debian/control");
    let control_abs = base_path.join(&control_rel);
    if !control_abs.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&control_abs)?;
    let Ok(control) = Control::from_str(&content) else {
        return Ok(Vec::new());
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    // Already at or above the minimum?
    let mut build_depends = source.build_depends().unwrap_or_default();
    let original = build_depends.to_string();
    let version = debversion::Version::from_str(&format!("{}~", minimum_version))
        .map_err(|e| FixerError::Other(format!("Failed to parse version: {:?}", e)))?;
    build_depends.ensure_minimum_version("debhelper", &version);
    if build_depends.to_string() == original {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "no-versioned-debhelper-prerequisite",
        vec![minimum_version.to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        format!(
            "Bump debhelper dependency to >= {}, since that's what is used in debian/compat.",
            minimum_version
        ),
        vec![Action::Deb822(Deb822Action::EnsureRelation {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            entry: format!("debhelper (>= {}~)", minimum_version),
        })],
    )])
}

declare_fixer! {
    name: "package-needs-versioned-debhelper-build-depends",
    tags: ["no-versioned-debhelper-prerequisite"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_bump_debhelper() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("compat"), "12\n").unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: blah\nMaintainer: Joe <joe@example.com>\nBuild-Depends: debhelper (>= 9), pkg-config\n\nPackage: blah\nDescription: blah\n blah\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\nMaintainer: Joe <joe@example.com>\nBuild-Depends: debhelper (>= 12~), pkg-config\n\nPackage: blah\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_no_change_when_already_correct() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("compat"), "12\n").unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nMaintainer: Joe <joe@example.com>\nBuild-Depends: debhelper (>= 12~)\n\nPackage: blah\nDescription: blah\n blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_compat() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper\n\nPackage: blah\nDescription: blah\n blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
