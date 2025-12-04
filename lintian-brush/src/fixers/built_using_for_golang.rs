use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");
    let editor = TemplatedControlEditor::open(&control_path)?;

    // Check if this is a Go package by looking at Build-Depends
    let is_go_package = if let Some(source) = editor.source() {
        if let Some(build_depends) = source.build_depends() {
            build_depends.entries().any(|or_deps| {
                or_deps
                    .relations()
                    .any(|dep| dep.name() == "golang-go" || dep.name() == "golang-any")
            })
        } else {
            false
        }
    } else {
        return Err(FixerError::NoChanges);
    };

    if !is_go_package {
        return Err(FixerError::NoChanges);
    }

    // Get default architecture from source package
    let default_architecture = editor.source().and_then(|s| s.architecture());

    let mut added = Vec::new();
    let mut removed = Vec::new();

    for mut binary in editor.binaries() {
        let binary_name = binary.name().unwrap_or_default();
        let architecture = binary
            .architecture()
            .or_else(|| default_architecture.clone())
            .unwrap_or_else(|| "any".to_string());

        if architecture == "all" {
            // Remove ${misc:Built-Using} for arch:all packages
            if let Some(built_using) = binary.built_using() {
                use debian_control::lossless::relations::Relations;
                let (mut relations, _) = Relations::parse_relaxed(&built_using.to_string(), true);

                let original_value = relations.to_string();
                relations.drop_substvar("${misc:Built-Using}");
                let new_value = relations.to_string();

                // Check if the substvar was actually removed
                if new_value != original_value {
                    if new_value.trim().is_empty() || relations.is_empty() {
                        binary.set_built_using(None);
                    } else {
                        let (new_relations, _) = Relations::parse_relaxed(&new_value, true);
                        binary.set_built_using(Some(&new_relations));
                    }
                    removed.push(binary_name.clone());
                }
            }
        } else {
            // Add ${misc:Built-Using} for non-all architectures
            let built_using = binary
                .built_using()
                .map(|b| b.to_string())
                .unwrap_or_default();
            use debian_control::lossless::relations::Relations;
            let (mut relations, _) = Relations::parse_relaxed(&built_using, true);

            // Check if ${misc:Built-Using} is already present
            let has_misc_built_using = relations.entries().any(|or_deps| {
                or_deps
                    .relations()
                    .any(|dep| dep.name() == "${misc:Built-Using}")
            });

            if !has_misc_built_using {
                relations.ensure_substvar("${misc:Built-Using}").unwrap();
                binary.set_built_using(Some(&relations));
                added.push(binary_name.clone());
            }
        }
    }

    if added.is_empty() && removed.is_empty() {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    let (description, fixed_tags) = if !added.is_empty() && !removed.is_empty() {
        (
            format!(
                "Added ${{misc:Built-Using}} to {} and removed it from {}.",
                added.join(", "),
                removed.join(", ")
            ),
            vec![
                "missing-built-using-field-for-golang-package".to_string(),
                "built-using-field-on-arch-all-package".to_string(),
            ],
        )
    } else if !added.is_empty() {
        (
            format!(
                "Add missing ${{misc:Built-Using}} to Built-Using on {}.",
                added.join(", ")
            ),
            vec!["missing-built-using-field-for-golang-package".to_string()],
        )
    } else {
        (
            format!(
                "Remove unnecessary ${{misc:Built-Using}} for {}",
                removed.join(", ")
            ),
            vec!["built-using-field-on-arch-all-package".to_string()],
        )
    };

    Ok(FixerResult::builder(description)
        .fixed_tags(fixed_tags)
        .build())
}

declare_fixer! {
    name: "built-using-for-golang",
    tags: ["built-using-for-golang"],
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
    fn test_add_built_using_for_golang_package() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: blah
Architecture: any
Build-Depends: golang-go

Package: blah
Description: blah
 blah
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "blah", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        let expected = r#"Source: blah
Architecture: any
Build-Depends: golang-go

Package: blah
Built-Using: ${misc:Built-Using}
Description: blah
 blah
"#;
        assert_eq!(updated_content, expected);
    }

    #[test]
    fn test_remove_built_using_for_arch_all() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: blah
Architecture: any
Build-Depends: golang-go

Package: blah
Architecture: all
Built-Using: ${misc:Built-Using}
Description: blah
 blah
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "blah", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        let expected = r#"Source: blah
Architecture: any
Build-Depends: golang-go

Package: blah
Architecture: all
Description: blah
 blah
"#;
        assert_eq!(updated_content, expected);
    }

    #[test]
    fn test_no_changes_for_non_go_package() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: blah
Architecture: any

Package: blah
Architecture: all
Built-Using: ${misc:Built-Using}
Description: blah
 blah
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "blah", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_unrelated_built_using() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: blah
Architecture: any
Build-Depends: golang-go

Package: blah
Architecture: all
Built-Using: ${w32:Built-Using}
Description: blah
 blah
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "blah", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Verify the other Built-Using is preserved
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(updated_content, control_content);
    }

    #[test]
    fn test_detects_golang_any() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: blah
Architecture: any
Build-Depends: golang-any

Package: blah
Description: blah
 blah
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "blah", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        let expected = r#"Source: blah
Architecture: any
Build-Depends: golang-any

Package: blah
Built-Using: ${misc:Built-Using}
Description: blah
 blah
"#;
        assert_eq!(updated_content, expected);
    }
}
