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

    // If source already has Priority, exit without changes
    if let Some(source) = editor.source() {
        if source.as_deb822().get("Priority").is_some() {
            return Err(FixerError::NoChanges);
        }
    }

    let mut binary_priorities = HashSet::new();
    let mut updated = HashMap::new();

    // Collect binaries to process
    let binaries: Vec<_> = editor.binaries().collect();

    for mut binary in binaries {
        let paragraph = binary.as_mut_deb822();
        let package_name = paragraph.get("Package").unwrap_or_default().to_string();

        if let Some(priority) = paragraph.get("Priority") {
            binary_priorities.insert(priority.to_string());
        } else {
            // Set priority to "optional" for binaries without it
            paragraph.set("Priority", "optional");
            binary_priorities.insert("optional".to_string());
            updated.insert(package_name, "optional".to_string());
        }
    }

    // If all binaries have the same priority, move it to source
    if binary_priorities.len() == 1 {
        let common_priority = binary_priorities.iter().next().unwrap().clone();

        // Set priority in source
        if let Some(mut source) = editor.source() {
            source.as_mut_deb822().set("Priority", &common_priority);
        }

        // Remove priority from all binaries
        let binaries: Vec<_> = editor.binaries().collect();
        for mut binary in binaries {
            binary.as_mut_deb822().remove("Priority");
        }

        editor.commit()?;

        let mut result_builder = FixerResult::builder(
            "Set priority in source stanza, since it is the same for all packages.",
        )
        .certainty(crate::Certainty::Confident);

        // Only add fixed tags if we actually added Priority to some binaries
        if !updated.is_empty() {
            result_builder = result_builder.fixed_tags(vec!["recommended-field"]);
        }

        return Ok(result_builder.build());
    } else if !updated.is_empty() {
        editor.commit()?;

        let packages_str: Vec<String> = updated
            .iter()
            .map(|(pkg, prio)| format!("{} ({})", pkg, prio))
            .collect();

        return Ok(FixerResult::builder(format!(
            "Set priority for binary packages {:?}.",
            packages_str
        ))
        .fixed_tags(vec!["recommended-field"])
        .build());
    }

    Err(FixerError::NoChanges)
}

declare_fixer! {
    name: "no-priority-field",
    tags: ["recommended-field"],
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
    fn test_missing_priority() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: foo\n\nPackage: blah\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "foo", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Source: foo\nPriority: optional"));
        assert!(!updated_content.contains("Package: blah\nPriority"));
    }

    #[test]
    fn test_common_priority() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content =
            "Source: foo\n\nPackage: foo\nPriority: optional\n\nPackage: foo-doc\nPriority: optional\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "foo", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Source: foo\nPriority: optional"));
        assert!(!updated_content.contains("Package: foo\nPriority"));
        assert!(!updated_content.contains("Package: foo-doc\nPriority"));
    }

    #[test]
    fn test_already_set_in_source() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: foo\nPriority: optional\n\nPackage: foo\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "foo", &version, &Default::default());
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
