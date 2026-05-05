use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const MINIMUM_DEBHELPER_VERSION: &str = "9.20160709";

const FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

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

    let mut drop_actions: Vec<Action> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for field in FIELDS {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        let (relations, _errors) =
            debian_control::lossless::relations::Relations::parse_relaxed(&value, true);
        let has_dh_systemd = relations.entries().any(|e| {
            e.relations()
                .any(|r| r.try_name().as_deref() == Some("dh-systemd"))
        });
        if !has_dh_systemd {
            continue;
        }

        let issue = LintianIssue::source_with_info(
            "build-depends-on-obsolete-package",
            vec![format!("{}: dh-systemd", field)],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "Depend on newer debhelper (>= 9.20160709) rather than dh-systemd.",
            Vec::new(),
        ));
        drop_actions.push(Action::Deb822(Deb822Action::DropRelation {
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Source,
            field: (*field).to_string(),
            package: "dh-systemd".into(),
        }));
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    // Bundle all the actions onto the first diagnostic so the applier
    // performs them in one pass.
    let mut all_actions = drop_actions;
    all_actions.push(Action::Deb822(Deb822Action::EnsureRelation {
        file: control_rel,
        paragraph: ParagraphSelector::Source,
        field: "Build-Depends".into(),
        entry: format!("debhelper (>= {})", MINIMUM_DEBHELPER_VERSION),
    }));
    diagnostics[0].plans[0].actions = all_actions;

    Ok(diagnostics)
}

declare_fixer! {
    name: "build-depends-on-obsolete-package",
    tags: ["build-depends-on-obsolete-package"],
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
    fn test_remove_dh_systemd() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: mypackage\nBuild-Depends: debhelper (>= 9), dh-systemd\n\nPackage: mypackage\nArchitecture: any\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: mypackage\nBuild-Depends: debhelper (>= 9.20160709)\n\nPackage: mypackage\nArchitecture: any\n",
        );
    }

    #[test]
    fn test_no_dh_systemd() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: mypackage\nBuild-Depends: debhelper (>= 9)\n\nPackage: mypackage\nArchitecture: any\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_remove_from_build_depends_indep() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: mypackage\nBuild-Depends: debhelper (>= 9)\nBuild-Depends-Indep: dh-systemd\n\nPackage: mypackage\nArchitecture: any\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: mypackage\nBuild-Depends: debhelper (>= 9.20160709)\n\nPackage: mypackage\nArchitecture: any\n",
        );
    }
}
