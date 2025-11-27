use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::control::TemplatedControlEditor;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    // Get all source fields and their values
    let source_fields: HashMap<String, String> = if let Some(source) = editor.source() {
        source
            .as_deb822()
            .keys()
            .map(|k| {
                (
                    k.to_string(),
                    source.as_deb822().get(&k).unwrap().to_string(),
                )
            })
            .collect()
    } else {
        return Err(FixerError::NoChanges);
    };

    let mut removed: HashMap<String, HashSet<String>> = HashMap::new();

    // Process all binary packages
    let binaries: Vec<_> = editor.binaries().collect();
    for mut binary in binaries {
        let paragraph = binary.as_mut_deb822();
        let package_name = paragraph.get("Package").unwrap_or_default().to_string();

        // Check all fields in the binary package
        let fields_to_check: Vec<(String, String)> = paragraph
            .keys()
            .map(|k| (k.to_string(), paragraph.get(&k).unwrap().to_string()))
            .collect();

        for (field, value) in fields_to_check {
            if let Some(source_value) = source_fields.get(&field) {
                if source_value == &value {
                    // This field in the binary package duplicates the source
                    paragraph.remove(&field);
                    removed
                        .entry(field.clone())
                        .or_default()
                        .insert(package_name.clone());
                }
            }
        }
    }

    if removed.is_empty() {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    // Build the result message
    let message = if removed.len() == 1 {
        let (field, binary_packages) = removed.iter().next().unwrap();
        let mut packages: Vec<_> = binary_packages.iter().cloned().collect();
        packages.sort();
        format!(
            "Remove field {} on binary package{} {} that duplicates source.",
            field,
            if packages.len() != 1 { "s" } else { "" },
            packages.join(", ")
        )
    } else {
        let mut message = "Remove fields on binary packages that duplicate source.".to_string();
        let mut sorted_fields: Vec<_> = removed.iter().collect();
        sorted_fields.sort_by_key(|(field, _)| field.as_str());

        for (field, packages) in sorted_fields {
            let mut package_list: Vec<_> = packages.iter().cloned().collect();
            package_list.sort();
            for package in package_list {
                message.push_str(&format!("\n+ Field {} from {}.", field, package));
            }
        }
        message
    };

    Ok(FixerResult::builder(&message)
        .fixed_tags(vec!["installable-field-mirrors-source"])
        .build())
}

declare_fixer! {
    name: "binary-control-field-duplicates-source",
    tags: ["installable-field-mirrors-source"],
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
    fn test_removes_duplicate_priority() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: blah\nSection: net\nPriority: optional\n\nPackage: blah\nSection: vcs\nPriority: optional\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "blah", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        // Priority should be removed from binary package
        let lines: Vec<&str> = updated_content.lines().collect();
        let package_start = lines.iter().position(|&l| l == "Package: blah").unwrap();
        assert!(!lines[package_start..]
            .iter()
            .any(|l| l.starts_with("Priority:")));
        // But Section should still be there (it's different)
        assert!(lines[package_start..]
            .iter()
            .any(|l| l.starts_with("Section: vcs")));
    }

    #[test]
    fn test_removes_multiple_duplicate_fields() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: blah\nSection: net\nPriority: optional\n\nPackage: blah\nSection: net\nPriority: optional\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "blah", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        // Both Priority and Section should be removed from binary package
        let lines: Vec<&str> = updated_content.lines().collect();
        let package_start = lines.iter().position(|&l| l == "Package: blah").unwrap();
        assert!(!lines[package_start..]
            .iter()
            .any(|l| l.starts_with("Priority:") || l.starts_with("Section:")));
    }

    #[test]
    fn test_no_change_when_fields_differ() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content =
            "Source: blah\nSection: net\nPriority: optional\n\nPackage: blah\nSection: vcs\nPriority: extra\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "blah", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_file() {
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
