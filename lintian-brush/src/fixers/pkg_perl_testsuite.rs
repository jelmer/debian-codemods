use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, PackageType};
use debian_changelog::parseaddr;
use std::path::{Path, PathBuf};

const PKG_PERL_EMAIL: &str = "pkg-perl-maintainers@lists.alioth.debian.org";
const TESTSUITE_VALUE: &str = "autopkgtest-pkg-perl";

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    // If debian/tests/control exists, the Testsuite header is redundant.
    // See https://bugs.debian.org/982871
    if ws.read_file(Path::new("debian/tests/control"))?.is_some() {
        return Ok(Vec::new());
    }

    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    let Some(maintainer) = source.get("Maintainer") else {
        return Ok(Vec::new());
    };
    let (_name, email) = parseaddr(&maintainer);
    if email != PKG_PERL_EMAIL {
        return Ok(Vec::new());
    }

    if source.get("Testsuite").as_deref().map(str::trim) == Some(TESTSUITE_VALUE) {
        return Ok(Vec::new());
    }

    let issue = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        tag: Some("team/pkg-perl/testsuite/no-testsuite-header".to_string()),
        info: Some("autopkgtest".to_string()),
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Set Testsuite header for perl package.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Testsuite".into(),
            value: TESTSUITE_VALUE.into(),
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "pkg-perl-testsuite",
    tags: ["team/pkg-perl/testsuite/no-testsuite-header"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/tests/control",
            paragraph_key: "Tests",
            field: "*",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/tests/control",
            paragraph_key: "Test-Command",
            field: "*",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Maintainer",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Testsuite",
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
        adapter.apply(base, "libfoo-perl", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_sets_testsuite_for_pkg_perl() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\n\nPackage: libfoo-perl\nDescription: test\n",
        )
        .unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(result.description, "Set Testsuite header for perl package.");

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\nTestsuite: autopkgtest-pkg-perl\n\nPackage: libfoo-perl\nDescription: test\n",
        );
    }

    #[test]
    fn test_no_change_when_testsuite_already_set() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\nTestsuite: autopkgtest-pkg-perl\n\nPackage: libfoo-perl\nDescription: test\n",
        )
        .unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_change_when_not_pkg_perl() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: libfoo-perl\nMaintainer: Someone Else <someone@example.com>\n\nPackage: libfoo-perl\nDescription: test\n",
        )
        .unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_change_when_no_control() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
