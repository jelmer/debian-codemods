use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    if base_path.join("debian/debcargo.toml").exists() {
        return Ok(Vec::new());
    }

    let format_path = base_path.join("debian/source/format");
    let format = if format_path.exists() {
        std::fs::read_to_string(&format_path)?.trim().to_string()
    } else {
        String::new()
    };
    if format == "3.0 (quilt)" {
        return Ok(Vec::new());
    }

    if !base_path.join("debian/patches/series").exists() {
        return Ok(Vec::new());
    }

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

    // Skip if quilt is already in Build-Depends.
    if let Some(value) = source.as_deb822().get("Build-Depends") {
        let (relations, _errors) =
            debian_control::lossless::relations::Relations::parse_relaxed(&value, true);
        if relations.entries().any(|e| {
            e.relations()
                .any(|r| r.try_name().as_deref() == Some("quilt"))
        }) {
            return Ok(Vec::new());
        }
    }

    let issue = LintianIssue::source_with_info(
        "quilt-series-but-no-build-dep",
        vec!["[debian/patches/series]".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Add missing dependency on quilt.",
        vec![Action::Deb822(Deb822Action::EnsureRelation {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            entry: "quilt".into(),
        })],
    )])
}

declare_fixer! {
    name: "quilt-series-but-no-build-dep",
    tags: ["quilt-series-but-no-build-dep"],
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
    fn test_simple() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        let patches = debian.join("patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "01-foo.patch\n").unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: blah\nMaintainer: Joe Example <joe@example.com>\nBuild-Depends: debhelper\n\nPackage: blah\nDescription: blah blah\n Blah blah\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\nMaintainer: Joe Example <joe@example.com>\nBuild-Depends: debhelper, quilt\n\nPackage: blah\nDescription: blah blah\n Blah blah\n",
        );
    }

    #[test]
    fn test_no_change_when_quilt_already_present() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        let patches = debian.join("patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "01-foo.patch\n").unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper, quilt\n\nPackage: blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_3_0_quilt() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        let patches = debian.join("patches");
        let source = debian.join("source");
        fs::create_dir_all(&patches).unwrap();
        fs::create_dir_all(&source).unwrap();
        fs::write(patches.join("series"), "01-foo.patch\n").unwrap();
        fs::write(source.join("format"), "3.0 (quilt)\n").unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper\n\nPackage: blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_series() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper\n\nPackage: blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
