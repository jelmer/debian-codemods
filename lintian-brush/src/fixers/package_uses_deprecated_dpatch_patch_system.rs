use crate::{declare_fixer, Certainty, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_control::lossless::relations::Relations;
use std::path::Path;

fn remove_dpatch_from_field(field_value: &str) -> Option<String> {
    if field_value.is_empty() {
        return None;
    }

    let (mut relations, _) = Relations::parse_relaxed(field_value, true);
    let mut entries_to_remove = Vec::new();

    for (idx, entry) in relations.entries().enumerate() {
        let mut to_remove_relations = Vec::new();

        for (rel_idx, relation) in entry.relations().enumerate() {
            if relation.name() == "dpatch" {
                to_remove_relations.push(rel_idx);
            }
        }

        // Remove relations in reverse order
        for rel_idx in to_remove_relations.into_iter().rev() {
            entry.remove_relation(rel_idx);
        }

        // Mark empty entries for removal
        if entry.relations().count() == 0 {
            entries_to_remove.push(idx);
        }
    }

    if entries_to_remove.is_empty() {
        return None;
    }

    // Remove empty entries in reverse order
    for idx in entries_to_remove.into_iter().rev() {
        relations.remove_entry(idx);
    }

    let new_contents = relations.to_string();
    if new_contents.trim().is_empty() || relations.is_empty() {
        Some(String::new())
    } else {
        Some(new_contents)
    }
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    // Read debian/control
    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Check if debian/patches directory exists
    let patches_dir = base_path.join("debian/patches");
    if !patches_dir.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    let mut changes_made = Vec::new();
    let mut has_dpatch = false;

    if let Some(mut source) = editor.source() {
        let source_para = source.as_mut_deb822();

        // Check and update Build-Depends
        if let Some(bd) = source_para.get("Build-Depends") {
            let bd_str = bd.to_string();
            if let Some(new_value) = remove_dpatch_from_field(&bd_str) {
                has_dpatch = true;
                if new_value.is_empty() {
                    source_para.remove("Build-Depends");
                } else {
                    source_para.set("Build-Depends", &new_value);
                }
                changes_made.push("Remove dpatch from Build-Depends");
            }
        }

        // Check and update Build-Depends-Indep
        if let Some(bdi) = source_para.get("Build-Depends-Indep") {
            let bdi_str = bdi.to_string();
            if let Some(new_value) = remove_dpatch_from_field(&bdi_str) {
                has_dpatch = true;
                if new_value.is_empty() {
                    source_para.remove("Build-Depends-Indep");
                } else {
                    source_para.set("Build-Depends-Indep", &new_value);
                }
                changes_made.push("Remove dpatch from Build-Depends-Indep");
            }
        }

        // Check and update Build-Depends-Arch
        if let Some(bda) = source_para.get("Build-Depends-Arch") {
            let bda_str = bda.to_string();
            if let Some(new_value) = remove_dpatch_from_field(&bda_str) {
                has_dpatch = true;
                if new_value.is_empty() {
                    source_para.remove("Build-Depends-Arch");
                } else {
                    source_para.set("Build-Depends-Arch", &new_value);
                }
                changes_made.push("Remove dpatch from Build-Depends-Arch");
            }
        }

        if !has_dpatch {
            return Err(FixerError::NoChanges);
        }

        let issue =
            LintianIssue::source_with_info("package-uses-deprecated-dpatch-patch-system", vec![]);

        if !issue.should_fix(base_path) {
            return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
        }

        // Write back control file
        drop(source);
        editor.commit()?;

        // Update source format to 3.0 (quilt)
        let source_format_dir = base_path.join("debian/source");
        std::fs::create_dir_all(&source_format_dir)?;

        let format_path = source_format_dir.join("format");
        std::fs::write(&format_path, "3.0 (quilt)\n")?;
        changes_made.push("Set source format to 3.0 (quilt)");

        // Rename 00list to series if it exists
        let list_file = patches_dir.join("00list");
        let series_file = patches_dir.join("series");

        if list_file.exists() && !series_file.exists() {
            std::fs::rename(&list_file, &series_file)?;
            changes_made.push("Rename debian/patches/00list to series");
        }

        Ok(FixerResult::builder(&format!(
            "Migrate from dpatch to 3.0 (quilt) source format. {}",
            changes_made.join(". ")
        ))
        .fixed_issues(vec![issue])
        .certainty(Certainty::Certain)
        .build())
    } else {
        Err(FixerError::NoChanges)
    }
}

declare_fixer! {
    name: "package-uses-deprecated-dpatch-patch-system",
    tags: ["package-uses-deprecated-dpatch-patch-system"],
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
    fn test_migrates_from_dpatch() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let patches_dir = debian_dir.join("patches");
        fs::create_dir_all(&patches_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Test User <test@example.com>
Build-Depends: debhelper (>= 11), dpatch

Package: test-package
Architecture: any
Description: Test package
 A test package.
"#;
        fs::write(debian_dir.join("control"), control_content).unwrap();

        // Create a 00list file
        fs::write(patches_dir.join("00list"), "01-test.patch\n").unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.description.contains("Migrate from dpatch"));

        // Check dpatch was removed from Build-Depends
        let control = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(!control.contains("dpatch"));

        // Check source format was created
        let format = fs::read_to_string(debian_dir.join("source/format")).unwrap();
        assert_eq!(format.trim(), "3.0 (quilt)");

        // Check 00list was renamed to series
        assert!(!patches_dir.join("00list").exists());
        assert!(patches_dir.join("series").exists());
    }

    #[test]
    fn test_no_changes_when_no_dpatch() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Test User <test@example.com>
Build-Depends: debhelper (>= 11)

Package: test-package
Architecture: any
Description: Test package
 A test package.
"#;
        fs::write(debian_dir.join("control"), control_content).unwrap();

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
    fn test_no_changes_when_no_patches_dir() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Maintainer: Test User <test@example.com>
Build-Depends: debhelper (>= 11), dpatch

Package: test-package
Architecture: any
Description: Test package
 A test package.
"#;
        fs::write(debian_dir.join("control"), control_content).unwrap();

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
    fn test_remove_dpatch_from_field() {
        // Test simple removal
        let result = remove_dpatch_from_field("debhelper (>= 11), dpatch");
        assert_eq!(result, Some("debhelper (>= 11)".to_string()));

        // Test dpatch only
        let result = remove_dpatch_from_field("dpatch");
        assert_eq!(result, Some(String::new()));

        // Test no dpatch
        let result = remove_dpatch_from_field("debhelper (>= 11)");
        assert_eq!(result, None);

        // Test empty
        let result = remove_dpatch_from_field("");
        assert_eq!(result, None);
    }
}
