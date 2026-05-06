use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, FilesystemAction, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, PackageType};
use debian_analyzer::debhelper::get_sequences;
use std::path::{Path, PathBuf};

const CERTAINTY: Certainty = Certainty::Possible;

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

    let sequences: Vec<String> = get_sequences(&source).collect();
    if !sequences.iter().any(|s| s == "nodejs") {
        return Ok(Vec::new());
    }

    let (test_runner, build_dep): (&str, &str) =
        if ws.read_file(Path::new("test/node.js"))?.is_some() {
            ("mocha test/node.js\n", "mocha <!nocheck>")
        } else if ws.read_file(Path::new("test.js"))?.is_some() {
            ("tape test.js\n", "node-tape <!nocheck>")
        } else {
            return Ok(Vec::new());
        };

    let issue = LintianIssue {
        package: source.as_deb822().get("Source").map(|s| s.to_string()),
        package_type: Some(PackageType::Source),
        tag: Some("pkg-js-tools-test-is-missing".to_string()),
        info: Some("debian/tests/pkg-js/test".to_string()),
    };

    let actions = vec![
        Action::Filesystem(FilesystemAction::Write {
            file: PathBuf::from("debian/tests/pkg-js/test"),
            content: test_runner.as_bytes().to_vec(),
        }),
        Action::Deb822(Deb822Action::EnsureRelation {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            entry: build_dep.to_string(),
        }),
    ];

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Add autopkgtest for node.",
        actions,
    )
    .with_certainty(CERTAINTY)])
}

declare_detector! {
    name: "pkg-js-tools-test-is-missing",
    tags: ["pkg-js-tools-test-is-missing"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_no_control() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_nodejs_sequence() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-pkg\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: test-pkg\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_nodejs_sequence_with_test_js() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: node-blah\nBuild-Depends: debhelper-compat (= 13)\n , dh-sequence-nodejs\n\nPackage: node-blah\n",
        )
        .unwrap();
        fs::write(tmp.path().join("test.js"), "").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/tests/pkg-js/test")).unwrap(),
            "tape test.js\n"
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: node-blah\nBuild-Depends: debhelper-compat (= 13)\n , dh-sequence-nodejs\n , node-tape <!nocheck>\n\nPackage: node-blah\n",
        );
    }
}
