use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DEPENDENCY_FIELDS: &[&str] = &[
    "Depends",
    "Pre-Depends",
    "Recommends",
    "Suggests",
    "Enhances",
    "Breaks",
    "Conflicts",
];

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
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
                Visibility::Warning,
                vec![field.to_string()],
            );
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    "Package depends on itself.",
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

declare_detector! {
    name: "circular-installation-prerequisite",
    tags: ["circular-installation-prerequisite"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Pre-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Recommends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Suggests",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Enhances",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Breaks",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Conflicts",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
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
