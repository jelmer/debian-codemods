use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut made_changes = false;

    // Only process the source paragraph
    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        // Check if both Maintainer and Uploaders fields exist
        if let (Some(maintainer), Some(uploaders)) =
            (paragraph.get("Maintainer"), paragraph.get("Uploaders"))
        {
            // Split uploaders by comma and check if maintainer is in the list
            let uploaders_list: Vec<String> =
                uploaders.split(',').map(|s| s.trim().to_string()).collect();

            // Check if maintainer is in uploaders list
            if uploaders_list.contains(&maintainer) {
                // Remove maintainer from uploaders
                let new_uploaders: Vec<String> = uploaders_list
                    .into_iter()
                    .filter(|u| u != &maintainer)
                    .collect();

                if new_uploaders.is_empty() {
                    // If no uploaders left, remove the field entirely
                    paragraph.remove("Uploaders");
                } else {
                    // Otherwise, update with the filtered list
                    paragraph.set("Uploaders", &new_uploaders.join(", "));
                }

                made_changes = true;
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    Ok(FixerResult::builder("Remove maintainer from uploaders.")
        .fixed_tags(vec!["maintainer-also-in-uploaders"])
        .build())
}

declare_fixer! {
    name: "maintainer-also-in-uploaders",
    tags: ["maintainer-also-in-uploaders"],
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
    fn test_maintainer_in_uploaders() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nMaintainer: John Doe <john@example.com>\nUploaders: John Doe <john@example.com>, Jane Smith <jane@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove maintainer from uploaders.");

        let content = fs::read_to_string(&control_path).unwrap();
        let expected = "Source: test\nMaintainer: John Doe <john@example.com>\nUploaders: Jane Smith <jane@example.com>\n\nPackage: test\nDescription: Test\n Test package\n";
        assert_eq!(content, expected);
    }

    #[test]
    fn test_maintainer_only_uploader() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nMaintainer: John Doe <john@example.com>\nUploaders: John Doe <john@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove maintainer from uploaders.");

        let content = fs::read_to_string(&control_path).unwrap();
        // Uploaders field should be completely removed
        let expected = "Source: test\nMaintainer: John Doe <john@example.com>\n\nPackage: test\nDescription: Test\n Test package\n";
        assert_eq!(content, expected);
    }

    #[test]
    fn test_maintainer_not_in_uploaders() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nMaintainer: John Doe <john@example.com>\nUploaders: Jane Smith <jane@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_uploaders_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nMaintainer: John Doe <john@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
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
    fn test_multiple_uploaders_with_maintainer() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nMaintainer: Bob <bob@example.com>\nUploaders: Alice <alice@example.com>, Bob <bob@example.com>, Charlie <charlie@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove maintainer from uploaders.");

        let content = fs::read_to_string(&control_path).unwrap();
        let expected = "Source: test\nMaintainer: Bob <bob@example.com>\nUploaders: Alice <alice@example.com>, Charlie <charlie@example.com>\n\nPackage: test\nDescription: Test\n Test package\n";
        assert_eq!(content, expected);
    }
}
