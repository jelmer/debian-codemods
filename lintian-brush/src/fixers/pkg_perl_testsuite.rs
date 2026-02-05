use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_changelog::parseaddr;
use std::path::Path;

const PKG_PERL_EMAIL: &str = "pkg-perl-maintainers@lists.alioth.debian.org";
const TESTSUITE_VALUE: &str = "autopkgtest-pkg-perl";

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Check if debian/tests/control exists - if so, the Testsuite header is redundant
    // See https://bugs.debian.org/982871
    let tests_control_path = base_path.join("debian/tests/control");
    if tests_control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    let source = editor.source().ok_or(FixerError::NoChanges)?;

    // Get the maintainer field
    let maintainer = source
        .as_deb822()
        .get("Maintainer")
        .ok_or(FixerError::NoChanges)?;

    // Parse the email from the maintainer field
    let (_name, email) = parseaddr(&maintainer);

    // Check if it's a pkg-perl maintained package
    if email != PKG_PERL_EMAIL {
        return Err(FixerError::NoChanges);
    }

    // Check if Testsuite is already set correctly
    if let Some(existing_testsuite) = source.as_deb822().get("Testsuite") {
        if existing_testsuite.trim() == TESTSUITE_VALUE {
            return Err(FixerError::NoChanges);
        }
    }

    // Check if there's an override for this issue
    let issue = LintianIssue {
        package: None,
        package_type: Some(crate::PackageType::Source),
        tag: Some("team/pkg-perl/testsuite/no-testsuite-header".to_string()),
        info: Some("autopkgtest".to_string()),
    };

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Set the Testsuite field
    if let Some(mut source) = editor.source() {
        source.as_mut_deb822().set("Testsuite", TESTSUITE_VALUE);
    }

    editor.commit()?;

    Ok(
        FixerResult::builder("Set Testsuite header for perl package.")
            .certainty(crate::Certainty::Certain)
            .fixed_issue(issue)
            .build(),
    )
}

declare_fixer! {
    name: "pkg-perl-testsuite",
    tags: ["team/pkg-perl/testsuite/no-testsuite-header"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_sets_testsuite_for_pkg_perl() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\n\nPackage: libfoo-perl\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "libfoo-perl",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Testsuite: autopkgtest-pkg-perl"));
    }

    #[test]
    fn test_no_change_when_testsuite_already_set() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\nTestsuite: autopkgtest-pkg-perl\n\nPackage: libfoo-perl\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "libfoo-perl",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_not_pkg_perl() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: libfoo-perl\nMaintainer: Someone Else <someone@example.com>\n\nPackage: libfoo-perl\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "libfoo-perl",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_control() {
        let temp_dir = TempDir::new().unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
