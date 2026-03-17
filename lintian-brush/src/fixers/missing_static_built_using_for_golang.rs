use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");
    let editor = TemplatedControlEditor::open(&control_path)?;

    // Check if this is a Go package by looking at Build-Depends
    let is_go_package = if let Some(source) = editor.source() {
        if let Some(build_depends) = source.build_depends() {
            build_depends.entries().any(|or_deps| {
                or_deps.relations().any(|dep| {
                    dep.try_name().as_deref() == Some("golang-go")
                        || dep.try_name().as_deref() == Some("golang-any")
                        || dep.try_name().as_deref() == Some("dh-golang")
                })
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
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for mut binary in editor.binaries() {
        let binary_name = binary.name().unwrap_or_default();
        let architecture = binary
            .architecture()
            .or_else(|| default_architecture.clone())
            .unwrap_or_else(|| "any".to_string());

        // Only add Static-Built-Using for non-all architectures
        if architecture != "all" {
            let static_built_using = binary
                .get("Static-Built-Using")
                .map(|s| s.to_string())
                .unwrap_or_default();

            // If the field exists and contains the substvar, skip this package
            if !static_built_using.is_empty()
                && static_built_using.contains("${misc:Static-Built-Using}")
            {
                continue;
            }

            use debian_control::lossless::relations::Relations;
            let (mut relations, _) = Relations::parse_relaxed(&static_built_using, true);

            // Check if ${misc:Static-Built-Using} is already present
            let has_misc_static_built_using = relations.entries().any(|or_deps| {
                or_deps
                    .relations()
                    .any(|dep| dep.try_name().as_deref() == Some("${misc:Static-Built-Using}"))
            });

            if !has_misc_static_built_using {
                // For missing field, use package stanza line
                let line_no = binary.as_deb822().line() + 1;

                let issue = LintianIssue {
                    package: Some(binary_name.clone()),
                    package_type: Some(crate::PackageType::Binary),
                    tag: Some("missing-static-built-using-field-for-golang-package".to_string()),
                    info: Some(format!(
                        "(in section for {}) [debian/control:{}]",
                        binary_name, line_no
                    )),
                };

                if issue.should_fix(base_path) {
                    relations
                        .ensure_substvar("${misc:Static-Built-Using}")
                        .unwrap();
                    binary.set("Static-Built-Using", &relations.to_string());
                    added.push(binary_name.clone());
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
            }
        }
    }

    if added.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    let description = format!(
        "Add missing ${{misc:Static-Built-Using}} to Static-Built-Using on {}.",
        added.join(", ")
    );

    Ok(FixerResult::builder(description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "missing-static-built-using-field-for-golang-package",
    tags: ["missing-static-built-using-field-for-golang-package"],
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
    fn test_add_static_built_using_for_golang_package() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: golang-foo
Architecture: any
Build-Depends: golang-go

Package: golang-foo
Architecture: any
Description: Foo library
 Test description
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "golang-foo", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Static-Built-Using: ${misc:Static-Built-Using}"));
    }

    #[test]
    fn test_no_changes_for_arch_all() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: golang-foo
Architecture: any
Build-Depends: golang-go

Package: golang-foo
Architecture: all
Description: Foo library
 Test description
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "golang-foo", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("Static-Built-Using"));
    }

    #[test]
    fn test_no_changes_for_non_go_package() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: python-foo
Architecture: any

Package: python-foo
Architecture: any
Description: Foo library
 Test description
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "python-foo", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_already_present() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: golang-foo
Architecture: any
Build-Depends: golang-go

Package: golang-foo
Architecture: any
Static-Built-Using: ${misc:Static-Built-Using}
Description: Foo library
 Test description
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "golang-foo", &version, &Default::default());
        if let Err(e) = &result {
            eprintln!("Result: {:?}", e);
        }
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_detects_golang_any() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: golang-foo
Architecture: any
Build-Depends: golang-any

Package: golang-foo
Architecture: any
Description: Foo library
 Test description
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "golang-foo", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Static-Built-Using: ${misc:Static-Built-Using}"));
    }

    #[test]
    fn test_detects_dh_golang() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: golang-foo
Architecture: any
Build-Depends: dh-golang

Package: golang-foo
Architecture: any
Description: Foo library
 Test description
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "golang-foo", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Static-Built-Using: ${misc:Static-Built-Using}"));
    }
}
