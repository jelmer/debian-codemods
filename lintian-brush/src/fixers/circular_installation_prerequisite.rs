use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const DEPENDENCY_FIELDS: &[&str] = &[
    "Depends",
    "Pre-Depends",
    "Recommends",
    "Suggests",
    "Enhances",
    "Breaks",
    "Conflicts",
];

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

    let mut diagnostics = Vec::new();
    for binary in control.binaries() {
        let Some(binary_name) = binary.as_deb822().get("Package") else {
            continue;
        };
        for field in DEPENDENCY_FIELDS {
            let Some(value) = binary.as_deb822().get(field) else {
                continue;
            };
            let (relations, _errors) =
                debian_control::lossless::relations::Relations::parse_relaxed(&value, true);
            if !relations.has_relation(&binary_name) {
                continue;
            }
            let issue = LintianIssue::source_with_info(
                "circular-installation-prerequisite",
                vec![field.to_string()],
            );
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    "Remove circular dependency on self in package.",
                    vec![Action::Deb822(Deb822Action::DropRelation {
                        file: control_rel.clone(),
                        paragraph: ParagraphSelector::Binary {
                            package: binary_name.clone(),
                        },
                        field: (*field).to_string(),
                        package: binary_name.clone(),
                    })],
                )
                .with_certainty(Certainty::Certain),
            );
        }
    }

    Ok(diagnostics)
}

declare_fixer! {
    name: "circular-installation-prerequisite",
    tags: ["circular-installation-prerequisite"],
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
    fn test_does_not_have_dependency() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let initial = "Source: test-package\nBuild-Depends: build-essential, debhelper-compat (= 13)\n\nPackage: test-package\nArchitecture: any\n";
        fs::write(debian.join("control"), initial).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(debian.join("control")).unwrap(), initial);
    }

    #[test]
    fn test_self_dep_single() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: blah\n\nPackage: blah-doc\nArchitecture: all\nDepends: blah-doc\nDescription: x\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\n\nPackage: blah-doc\nArchitecture: all\nDescription: x\n",
        );
    }

    #[test]
    fn test_self_dep_one_of_many() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: blah\n\nPackage: blah-doc\nArchitecture: all\nDepends: blah-doc, python3\nDescription: x\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\n\nPackage: blah-doc\nArchitecture: all\nDepends: python3\nDescription: x\n",
        );
    }
}
