use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::control::TemplatedControlEditor;
use std::collections::HashSet;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut made_changes = false;

    // Get source homepage
    let source_homepage = if let Some(source) = editor.source() {
        source.as_deb822().get("Homepage")
    } else {
        None
    };

    // Collect unique binary homepages
    let mut binary_homepages = HashSet::new();
    for binary in editor.binaries() {
        if let Some(homepage) = binary.as_deb822().get("Homepage") {
            if source_homepage.as_ref() != Some(&homepage) {
                binary_homepages.insert(homepage);
            }
        }
    }

    // First pass: Remove binary Homepage fields that match source Homepage
    if source_homepage.is_some() {
        for mut binary in editor.binaries() {
            let paragraph = binary.as_mut_deb822();
            if let Some(binary_homepage) = paragraph.get("Homepage") {
                if source_homepage.as_ref() == Some(&binary_homepage) {
                    paragraph.remove("Homepage");
                    made_changes = true;
                }
            }
        }
    }

    // Second pass: If no source homepage but all binaries have the same homepage,
    // move it to source
    if source_homepage.is_none() && binary_homepages.len() == 1 {
        let homepage = binary_homepages.iter().next().unwrap().clone();

        // Set homepage in source
        if let Some(mut source) = editor.source() {
            source.as_mut_deb822().set("Homepage", &homepage);
            made_changes = true;
        }

        // Remove homepage from all binaries
        for mut binary in editor.binaries() {
            let paragraph = binary.as_mut_deb822();
            if paragraph.get("Homepage").is_some() {
                paragraph.remove("Homepage");
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    Ok(
        FixerResult::builder("Set Homepage field in Source rather than Binary package.")
            .fixed_tags(vec!["homepage-in-binary-package"])
            .build(),
    )
}

declare_fixer! {
    name: "homepage-in-binary-package",
    tags: ["homepage-in-binary-package"],
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
    fn test_no_source_homepage_same_in_binaries() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nMaintainer: Joe <joe@example.com>\n\nPackage: blah1\nHomepage: https://www.example.com/blah\nDescription: blah\n\nPackage: blah2\nHomepage: https://www.example.com/blah\nDescription: blah2\n",
        )
        .unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(content.contains("Source: blah\nMaintainer: Joe <joe@example.com>\nHomepage: https://www.example.com/blah\n"));
        assert!(!content.contains("Package: blah1\nHomepage"));
        assert!(!content.contains("Package: blah2\nHomepage"));

        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Set Homepage field in Source rather than Binary package."
        );
    }

    #[test]
    fn test_source_homepage_matches_binary() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nMaintainer: Joe <joe@example.com>\nHomepage: https://www.example.com/blah\n\nPackage: blah1\nHomepage: https://www.example.com/blah\nDescription: blah\n\nPackage: blah2\nHomepage: https://www.example.com/blah2\nDescription: blah2\n",
        )
        .unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(!content.contains("Package: blah1\nHomepage"));
        assert!(content.contains("Package: blah2\nHomepage: https://www.example.com/blah2"));
    }

    #[test]
    fn test_no_change_when_different_homepages() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nMaintainer: Joe <joe@example.com>\n\nPackage: blah1\nHomepage: https://www.example.com/blah1\nDescription: blah\n\nPackage: blah2\nHomepage: https://www.example.com/blah2\nDescription: blah2\n",
        )
        .unwrap();

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

    #[test]
    fn test_no_change_when_no_homepage() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nMaintainer: Joe <joe@example.com>\n\nPackage: blah1\nDescription: blah\n",
        )
        .unwrap();

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
