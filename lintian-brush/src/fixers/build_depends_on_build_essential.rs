use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use std::path::PathBuf;

const FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep"];

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
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    let mut diagnostics = Vec::new();
    for field in FIELDS {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        let (relations, _errors) = Relations::parse_relaxed(&value, true);
        let has_build_essential = relations.entries().any(|e| {
            e.relations()
                .any(|r| r.try_name().as_deref() == Some("build-essential"))
        });
        if !has_build_essential {
            continue;
        }

        let issue = LintianIssue::source_with_info(
            "build-depends-on-build-essential",
            Visibility::Error,
            vec![field.to_string()],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "Source package depends on build-essential.",
                "Drop unnecessary dependency on build-essential.",
                vec![Action::Deb822(Deb822Action::DropRelation {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Source,
                    field: (*field).to_string(),
                    package: "build-essential".into(),
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "build-depends-on-build-essential",
    tags: ["build-depends-on-build-essential"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends-Indep",
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
    fn test_removes_build_essential_from_build_depends() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test-package\nBuild-Depends: build-essential, debhelper-compat (= 13)\n\nPackage: test-package\nArchitecture: any\nDepends: ${shlibs:Depends}, ${misc:Depends}\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-package\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: test-package\nArchitecture: any\nDepends: ${shlibs:Depends}, ${misc:Depends}\n",
        );
    }

    #[test]
    fn test_no_change_without_build_essential() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: test-package\nArchitecture: any\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
