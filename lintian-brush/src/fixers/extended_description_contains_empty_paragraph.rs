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

    // Process binary packages
    for mut binary in editor.binaries() {
        let paragraph = binary.as_mut_deb822();

        // Check if Description field exists
        if let Some(description) = paragraph.get("Description") {
            // Split into lines but preserve the line endings
            let lines: Vec<&str> = description.split('\n').collect();

            // Check if we have at least 2 lines and the second line is "."
            // (which represents an empty paragraph in debian control files)
            // Note: The leading space is stripped by the deb822 parser
            if lines.len() > 1 && lines[1] == "." {
                // Reconstruct the description without the empty paragraph at the start
                let mut new_lines = Vec::new();
                new_lines.push(lines[0]); // Keep the short description

                // Skip the empty paragraph (line 1) and add the rest
                for line in lines.iter().skip(2) {
                    new_lines.push(line);
                }

                // Join with newlines
                let new_description = new_lines.join("\n");

                paragraph.set("Description", &new_description);
                made_changes = true;
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    Ok(
        FixerResult::builder("Remove empty leading paragraph in Description.")
            .fixed_tags(vec!["extended-description-contains-empty-paragraph"])
            .build(),
    )
}

declare_fixer! {
    name: "extended-description-contains-empty-paragraph",
    tags: ["extended-description-contains-empty-paragraph"],
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
    fn test_empty_paragraph_at_start() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\n\nPackage: test\nDescription: This is a package\n .\n But it starts with an empty paragraph.\n .\n And then more.\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove empty leading paragraph in Description."
        );

        let content = fs::read_to_string(&control_path).unwrap();
        let expected = "Source: test\n\nPackage: test\nDescription: This is a package\n But it starts with an empty paragraph.\n .\n And then more.\n";
        assert_eq!(content, expected);
    }

    #[test]
    fn test_no_empty_paragraph() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\n\nPackage: test\nDescription: This is a package\n With a normal extended description.\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_empty_paragraph_not_at_start() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\n\nPackage: test\nDescription: This is a package\n With some text.\n .\n And then more after a separator.\n",
        )
        .unwrap();

        let result = run(base_path);
        // Should not change anything since the empty paragraph is not at the start
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Verify file hasn't changed
        let content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(content, "Source: test\n\nPackage: test\nDescription: This is a package\n With some text.\n .\n And then more after a separator.\n");
    }

    #[test]
    fn test_no_description_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\n\nPackage: test\n").unwrap();

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
    fn test_multiple_packages_with_empty_paragraph() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\n\nPackage: test1\nDescription: First package\n .\n Extended description.\n\nPackage: test2\nDescription: Second package\n .\n Another extended description.\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove empty leading paragraph in Description."
        );

        let content = fs::read_to_string(&control_path).unwrap();
        let expected = "Source: test\n\nPackage: test1\nDescription: First package\n Extended description.\n\nPackage: test2\nDescription: Second package\n Another extended description.\n";
        assert_eq!(content, expected);
    }
}
