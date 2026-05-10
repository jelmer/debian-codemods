use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::PathBuf;

const PACKAGE: &str = "libmodule-build-tiny-perl";
const TAG: &str = "libmodule-build-tiny-perl-needs-to-be-in-build-depends";

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
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
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            from_field: "Build-Depends-Indep".into(),
            to_field: "Build-Depends".into(),
            package: PACKAGE.into(),
        })],
    )])
}

declare_detector! {
    name: "libmodule-build-tiny-perl-needs-to-be-in-build-depends",
    tags: ["libmodule-build-tiny-perl-needs-to-be-in-build-depends"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
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
    use crate::workspace::DetectorAdapter;
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
    fn test_simple() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: libtest-example-perl\nSection: perl\nPriority: optional\nMaintainer: Joe Maintainer <joe@example.com>\nBuild-Depends: debhelper (>= 9)\nBuild-Depends-Indep: libclass-load-perl (>= 0.06)\n , libmodule-build-tiny-perl\n , perl\nStandards-Version: 3.9.6\n\nPackage: libtest-example-perl\nArchitecture: all\nDepends: ${misc:Depends}, ${perl:Depends}\n , libclass-load-perl (>= 0.06)\nDescription: Example perl library\n An example perl library.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Move libmodule-build-tiny-perl from Build-Depends-Indep to Build-Depends."
        );

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: libtest-example-perl\nSection: perl\nPriority: optional\nMaintainer: Joe Maintainer <joe@example.com>\nBuild-Depends: debhelper (>= 9), libmodule-build-tiny-perl\nBuild-Depends-Indep: libclass-load-perl (>= 0.06)\n , perl\nStandards-Version: 3.9.6\n\nPackage: libtest-example-perl\nArchitecture: all\nDepends: ${misc:Depends}, ${perl:Depends}\n , libclass-load-perl (>= 0.06)\nDescription: Example perl library\n An example perl library.\n"
        );
    }

    #[test]
    fn test_removes_build_depends_indep_when_empty() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: libtest-example-perl\nSection: perl\nPriority: optional\nMaintainer: Joe Maintainer <joe@example.com>\nBuild-Depends: debhelper (>= 9)\nBuild-Depends-Indep: libmodule-build-tiny-perl\nStandards-Version: 3.9.6\n\nPackage: libtest-example-perl\nArchitecture: all\nDepends: ${misc:Depends}\nDescription: Example perl library\n An example perl library.\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: libtest-example-perl\nSection: perl\nPriority: optional\nMaintainer: Joe Maintainer <joe@example.com>\nBuild-Depends: debhelper (>= 9), libmodule-build-tiny-perl\nStandards-Version: 3.9.6\n\nPackage: libtest-example-perl\nArchitecture: all\nDepends: ${misc:Depends}\nDescription: Example perl library\n An example perl library.\n",
        );
    }

    #[test]
    fn test_no_changes() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test\nBuild-Depends: debhelper (>= 9), libmodule-build-tiny-perl\n\nPackage: test\nDescription: Test\n",
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
