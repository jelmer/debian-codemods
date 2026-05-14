use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const PACKAGE: &str = "libmodule-build-perl";
const TAG: &str = "libmodule-build-perl-needs-to-be-in-build-depends";

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
    let Some(build_depends_indep) = source.build_depends_indep() else {
        return Ok(Vec::new());
    };
    if build_depends_indep.get_relation(PACKAGE).is_err() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(TAG, Visibility::Error, vec![]);
    Ok(vec![Diagnostic::with_actions(
        issue,
        format!(
            "{} is in Build-Depends-Indep but needs Build-Depends.",
            PACKAGE
        ),
        format!(
            "Move {} from Build-Depends-Indep to Build-Depends.",
            PACKAGE
        ),
        vec![Action::Deb822(Deb822Action::MoveRelation {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            from_field: "Build-Depends-Indep".into(),
            to_field: "Build-Depends".into(),
            package: PACKAGE.into(),
        })],
    )])
}

declare_detector! {
    name: "libmodule-build-perl-needs-to-be-in-build-depends",
    tags: ["libmodule-build-perl-needs-to-be-in-build-depends"],
    triggers: [
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
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_simple() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: libtest-mock-guard-perl\nSection: perl\nPriority: optional\nMaintainer: Joe Maintainer <joe@example.com>\nBuild-Depends: debhelper (>= 9)\nBuild-Depends-Indep: libclass-load-perl (>= 0.06)\n , libmodule-build-perl\n , perl\nStandards-Version: 3.9.6\n\nPackage: libtest-mock-guard-perl\nArchitecture: all\nDepends: ${misc:Depends}, ${perl:Depends}\n , libclass-load-perl (>= 0.06)\nDescription: Simple mock test library\n A mock test library.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Move libmodule-build-perl from Build-Depends-Indep to Build-Depends."
        );

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: libtest-mock-guard-perl\nSection: perl\nPriority: optional\nMaintainer: Joe Maintainer <joe@example.com>\nBuild-Depends: debhelper (>= 9), libmodule-build-perl\nBuild-Depends-Indep: libclass-load-perl (>= 0.06)\n , perl\nStandards-Version: 3.9.6\n\nPackage: libtest-mock-guard-perl\nArchitecture: all\nDepends: ${misc:Depends}, ${perl:Depends}\n , libclass-load-perl (>= 0.06)\nDescription: Simple mock test library\n A mock test library.\n"
        );
    }

    #[test]
    fn test_no_changes() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test\nBuild-Depends: debhelper (>= 9), libmodule-build-perl\n\nPackage: test\nDescription: Test\n",
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
