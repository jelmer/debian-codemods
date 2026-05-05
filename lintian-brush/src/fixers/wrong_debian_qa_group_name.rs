use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_changelog::parseaddr;
use std::path::{Path, PathBuf};

const QA_EMAIL: &str = "packages@qa.debian.org";
const QA_MAINTAINER: &str = "Debian QA Group <packages@qa.debian.org>";

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&control_path)?;
    let control: debian_control::lossless::Control =
        content.parse().map_err(|_| FixerError::NoChanges)?;

    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let Some(maintainer) = source.as_deb822().get("Maintainer") else {
        return Ok(Vec::new());
    };

    let (name_opt, email) = parseaddr(&maintainer);
    if email != QA_EMAIL || maintainer == QA_MAINTAINER {
        return Ok(Vec::new());
    }

    let name = name_opt.unwrap_or("Debian QA");
    let issue = LintianIssue::source_with_info(
        "faulty-debian-qa-group-phrase",
        vec![format!("Maintainer {} -> Debian QA Group", name)],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Fix Debian QA group name.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Maintainer".into(),
            value: QA_MAINTAINER.into(),
        })],
    )])
}

declare_fixer! {
    name: "wrong-debian-qa-group-name",
    tags: ["faulty-debian-qa-group-phrase"],
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
    fn test_wrong_qa_group_name() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: lintian-brush\nMaintainer: QA Folks <packages@qa.debian.org>\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, "Fix Debian QA group name.");

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: lintian-brush\nMaintainer: Debian QA Group <packages@qa.debian.org>\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        );
    }

    #[test]
    fn test_correct_qa_group_name() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nMaintainer: Debian QA Group <packages@qa.debian.org>\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert!(detect(base_path).unwrap().is_empty());
    }

    #[test]
    fn test_different_maintainer() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nMaintainer: John Doe <john@example.com>\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_maintainer_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert!(detect(temp_dir.path()).unwrap().is_empty());
    }

    #[test]
    fn test_various_wrong_qa_names() {
        let test_cases = vec![
            "QA Group <packages@qa.debian.org>",
            "Debian QA <packages@qa.debian.org>",
            "QA Team <packages@qa.debian.org>",
            "Orphaned <packages@qa.debian.org>",
        ];

        for wrong_name in test_cases {
            let temp_dir = TempDir::new().unwrap();
            let base_path = temp_dir.path();
            let debian_dir = base_path.join("debian");
            fs::create_dir(&debian_dir).unwrap();

            let control_path = debian_dir.join("control");
            fs::write(&control_path, format!("Source: test\nMaintainer: {}\n\nPackage: test\nDescription: Test\n Test package\n", wrong_name)).unwrap();

            let result = run_apply(base_path).unwrap();
            assert_eq!(result.description, "Fix Debian QA group name.");

            assert_eq!(
                fs::read_to_string(&control_path).unwrap(),
                "Source: test\nMaintainer: Debian QA Group <packages@qa.debian.org>\n\nPackage: test\nDescription: Test\n Test package\n",
            );
        }
    }

    #[test]
    fn test_diagnostic_carries_action() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();
        fs::write(
            debian_dir.join("control"),
            "Source: test\nMaintainer: QA <packages@qa.debian.org>\n\nPackage: test\n",
        )
        .unwrap();

        let diagnostics = detect(base_path).unwrap();
        assert_eq!(diagnostics.len(), 1);
        let diag = &diagnostics[0];
        assert_eq!(diag.message, "Fix Debian QA group name.");
        assert_eq!(diag.plans.len(), 1);
        assert_eq!(diag.plans[0].actions.len(), 1);
        assert_eq!(
            diag.plans[0].actions[0],
            Action::Deb822(Deb822Action::SetField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Source,
                field: "Maintainer".into(),
                value: QA_MAINTAINER.into(),
            })
        );
    }

    #[test]
    fn test_overridden_diagnostic() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();
        fs::write(
            debian_dir.join("control"),
            "Source: test\nMaintainer: QA <packages@qa.debian.org>\n\nPackage: test\n",
        )
        .unwrap();
        fs::create_dir(debian_dir.join("source")).unwrap();
        fs::write(
            debian_dir.join("source/lintian-overrides"),
            "faulty-debian-qa-group-phrase\n",
        )
        .unwrap();

        match run_apply(base_path).unwrap_err() {
            FixerError::NoChangesAfterOverrides(issues) => {
                assert_eq!(issues.len(), 1);
                assert_eq!(
                    issues[0].tag.as_deref(),
                    Some("faulty-debian-qa-group-phrase")
                );
            }
            other => panic!("expected NoChangesAfterOverrides, got {:?}", other),
        }
        // Control file untouched.
        assert_eq!(
            fs::read_to_string(debian_dir.join("control")).unwrap(),
            "Source: test\nMaintainer: QA <packages@qa.debian.org>\n\nPackage: test\n",
        );
    }
}
