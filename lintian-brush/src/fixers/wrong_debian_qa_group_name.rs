use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

const QA_MAINTAINER: &str = "Debian QA Group <packages@qa.debian.org>";

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut made_changes = false;

    // Only process the source paragraph (Maintainer only appears there)
    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        // Check if Maintainer field exists
        if let Some(maintainer) = paragraph.get("Maintainer") {
            // Parse the maintainer field to extract email
            // Simple parsing: look for email between < and >
            if let Some(start) = maintainer.find('<') {
                if let Some(end) = maintainer.find('>') {
                    let email = &maintainer[start + 1..end];

                    // Check if it's the QA email address
                    if email == "packages@qa.debian.org" {
                        // Check if the full maintainer string is not already correct
                        if maintainer != QA_MAINTAINER {
                            paragraph.set("Maintainer", QA_MAINTAINER);
                            made_changes = true;
                        }
                    }
                }
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    Ok(FixerResult::builder("Fix Debian QA group name.")
        .fixed_tags(vec!["faulty-debian-qa-group-phrase"])
        .build())
}

declare_fixer! {
    name: "wrong-debian-qa-group-name",
    tags: ["faulty-debian-qa-group-phrase"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_wrong_qa_group_name() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: lintian-brush\nMaintainer: QA Folks <packages@qa.debian.org>\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Fix Debian QA group name.");

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(content.contains("Maintainer: Debian QA Group <packages@qa.debian.org>"));
        assert!(!content.contains("QA Folks"));
    }

    #[test]
    fn test_correct_qa_group_name() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nMaintainer: Debian QA Group <packages@qa.debian.org>\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_different_maintainer() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nMaintainer: John Doe <john@example.com>\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
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

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
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

            let result = run(base_path).unwrap();
            assert_eq!(result.description, "Fix Debian QA group name.");

            let content = fs::read_to_string(&control_path).unwrap();
            assert!(content.contains("Maintainer: Debian QA Group <packages@qa.debian.org>"));
        }
    }
}
