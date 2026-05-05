use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::FixerError;
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
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
    let Some(build_depends_indep) = source.build_depends_indep() else {
        return Ok(Vec::new());
    };
    if build_depends_indep
        .get_relation("debhelper-compat")
        .is_err()
    {
        return Ok(Vec::new());
    }

    Ok(vec![Diagnostic::untagged(
        "Move debhelper-compat from Build-Depends-Indep to Build-Depends.",
        vec![Action::Deb822(Deb822Action::MoveRelation {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            from_field: "Build-Depends-Indep".into(),
            to_field: "Build-Depends".into(),
            package: "debhelper-compat".into(),
        })],
    )])
}

declare_fixer! {
    name: "debhelper-compat-wrong-field",
    tags: [],
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
    fn test_move_debhelper_compat() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: blah\nBuild-Depends-Indep: debhelper-compat (= 12)\nBuild-Depends: python3-dulwich\n\nPackage: blah\nDescription: blah\n blah blah\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Move debhelper-compat from Build-Depends-Indep to Build-Depends.",
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 12), python3-dulwich\n\nPackage: blah\nDescription: blah\n blah blah\n",
        );
    }

    #[test]
    fn test_no_change_when_not_in_indep() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper-compat (= 12)\n\nPackage: blah\nDescription: blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
