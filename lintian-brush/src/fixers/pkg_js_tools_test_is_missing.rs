use crate::{declare_fixer, Certainty, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_analyzer::debhelper::get_sequences;
use debian_analyzer::relations::ensure_some_version;
use std::fs;
use std::path::Path;

const CERTAINTY: Certainty = Certainty::Possible;

/// Check if the nodejs sequence is used
fn has_nodejs_sequence(editor: &TemplatedControlEditor) -> bool {
    if let Some(source) = editor.source() {
        let sequences: Vec<String> = get_sequences(&source).collect();
        sequences.contains(&"nodejs".to_string())
    } else {
        false
    }
}

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    if let Some(min_certainty) = preferences.minimum_certainty {
        if min_certainty > CERTAINTY {
            return Err(FixerError::NotCertainEnough(
                CERTAINTY,
                Some(min_certainty),
                vec![],
            ));
        }
    }

    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    if !has_nodejs_sequence(&editor) {
        return Err(FixerError::NoChanges);
    }

    let issue = LintianIssue {
        package: editor
            .source()
            .and_then(|s| s.as_deb822().get("Source"))
            .map(|s| s.to_string()),
        package_type: Some(crate::PackageType::Source),
        tag: Some("pkg-js-tools-test-is-missing".to_string()),
        info: Some("debian/tests/pkg-js/test".to_string()),
    };

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    let test_node_path = base_path.join("test/node.js");
    let test_js_path = base_path.join("test.js");

    let (test_runner, build_dep) = if test_node_path.exists() {
        ("mocha test/node.js\n", "mocha <!nocheck>")
    } else if test_js_path.exists() {
        ("tape test.js\n", "node-tape <!nocheck>")
    } else {
        return Err(FixerError::NoChanges);
    };

    // Create the test file
    let pkg_js_dir = base_path.join("debian/tests/pkg-js");
    fs::create_dir_all(&pkg_js_dir)?;
    let test_file = pkg_js_dir.join("test");
    fs::write(&test_file, test_runner)?;

    // Update Build-Depends
    if let Some(mut source) = editor.source() {
        let original_build_depends = source.build_depends().unwrap_or_default();
        let mut new_build_depends = original_build_depends;
        ensure_some_version(&mut new_build_depends, build_dep);
        source.set_build_depends(&new_build_depends);
    }

    editor.commit()?;

    Ok(FixerResult::builder("Add autopkgtest for node.")
        .certainty(CERTAINTY)
        .fixed_issue(issue)
        .build())
}

declare_fixer! {
    name: "pkg-js-tools-test-is-missing",
    tags: ["pkg-js-tools-test-is-missing"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_no_control() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_nodejs_sequence() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: test-pkg
Maintainer: Test User <test@example.com>
Build-Depends: debhelper-compat (= 13)

Package: test-pkg
Architecture: all
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_nodejs_sequence_with_test_node_js() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: node-test-pkg
Maintainer: Test User <test@example.com>
Build-Depends: debhelper-compat (= 13), dh-sequence-nodejs

Package: node-test-pkg
Architecture: all
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        // Create test/node.js
        let test_dir = base_path.join("test");
        fs::create_dir(&test_dir).unwrap();
        fs::write(test_dir.join("node.js"), "// test file\n").unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.description, "Add autopkgtest for node.");
        assert_eq!(result.certainty, Some(Certainty::Possible));

        // Check that the test file was created
        let test_file = base_path.join("debian/tests/pkg-js/test");
        assert!(test_file.exists());
        let content = fs::read_to_string(&test_file).unwrap();
        assert_eq!(content, "mocha test/node.js\n");

        // Check that mocha was added to Build-Depends
        let editor = TemplatedControlEditor::open(&control_path).unwrap();
        let source = editor.source().unwrap();
        let build_depends = source.build_depends().unwrap();
        assert!(build_depends.to_string().contains("mocha <!nocheck>"));
    }

    #[test]
    fn test_nodejs_sequence_with_test_js() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: node-test-pkg
Maintainer: Test User <test@example.com>
Build-Depends: debhelper-compat (= 13), dh-sequence-nodejs

Package: node-test-pkg
Architecture: all
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        // Create test.js
        fs::write(base_path.join("test.js"), "// test file\n").unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.description, "Add autopkgtest for node.");
        assert_eq!(result.certainty, Some(Certainty::Possible));

        // Check that the test file was created
        let test_file = base_path.join("debian/tests/pkg-js/test");
        assert!(test_file.exists());
        let content = fs::read_to_string(&test_file).unwrap();
        assert_eq!(content, "tape test.js\n");

        // Check that node-tape was added to Build-Depends
        let editor = TemplatedControlEditor::open(&control_path).unwrap();
        let source = editor.source().unwrap();
        let build_depends = source.build_depends().unwrap();
        assert!(build_depends.to_string().contains("node-tape <!nocheck>"));
    }

    #[test]
    fn test_nodejs_sequence_no_test_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: node-test-pkg
Maintainer: Test User <test@example.com>
Build-Depends: debhelper-compat (= 13), dh-sequence-nodejs

Package: node-test-pkg
Architecture: all
Description: Test package
 Test description
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
