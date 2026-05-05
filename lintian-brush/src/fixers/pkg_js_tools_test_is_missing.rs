use crate::diagnostic::{Action, Deb822Action, Diagnostic, FilesystemAction, ParagraphSelector};
use crate::{Certainty, FixerError, LintianIssue, PackageType};
use debian_analyzer::debhelper::get_sequences;
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const CERTAINTY: Certainty = Certainty::Possible;

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

    let sequences: Vec<String> = get_sequences(&source).collect();
    if !sequences.iter().any(|s| s == "nodejs") {
        return Ok(Vec::new());
    }

    let test_node_path = base_path.join("test/node.js");
    let test_js_path = base_path.join("test.js");

    let (test_runner, build_dep): (&str, &str) = if test_node_path.exists() {
        ("mocha test/node.js\n", "mocha <!nocheck>")
    } else if test_js_path.exists() {
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
            file: control_rel,
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

declare_fixer! {
    name: "pkg-js-tools-test-is-missing",
    tags: ["pkg-js-tools-test-is-missing"],
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
