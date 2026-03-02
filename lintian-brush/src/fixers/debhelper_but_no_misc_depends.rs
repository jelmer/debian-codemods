use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_analyzer::debhelper::get_debhelper_compat_level;
use debian_control::lossless::relations::Relations;
use std::path::Path;

fn uses_debhelper(build_depends: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(build_depends, true);

    for entry in relations.entries() {
        for relation in entry.relations() {
            let name = relation.name();
            if name == "debhelper" || name == "debhelper-compat" {
                return true;
            }
        }
    }

    false
}

fn has_misc_depends(field_value: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(field_value, true);

    // Check if ${misc:Depends} substvar is present
    let substvars: Vec<String> = relations.substvars().collect();
    substvars.iter().any(|s| s == "${misc:Depends}")
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Check debhelper compat level - skip fix for compat >= 14
    // See: https://bugs.debian.org/cgi-bin/bugreport.cgi?bug=1072700
    let compat_level = get_debhelper_compat_level(base_path)?;
    if let Some(level) = compat_level {
        if level >= 14 {
            return Err(FixerError::NoChanges);
        }
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut misc_depends_added = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Check if the source uses debhelper
    let uses_dh = if let Some(source) = editor.source() {
        if let Some(build_depends) = source.build_depends() {
            uses_debhelper(&build_depends.to_string())
        } else {
            false
        }
    } else {
        false
    };

    if !uses_dh {
        return Err(FixerError::NoChanges);
    }

    // Check each binary package
    for mut binary in editor.binaries() {
        let package_name = match binary.name() {
            Some(name) => name.to_string(),
            None => {
                tracing::debug!("Skipping binary package without name");
                continue;
            }
        };

        let depends = binary.depends().map(|d| d.to_string()).unwrap_or_default();
        let pre_depends = binary
            .as_deb822()
            .get("Pre-Depends")
            .map(|s| s.to_string())
            .unwrap_or_default();

        // Skip if already has ${misc:Depends} in either Depends or Pre-Depends
        if has_misc_depends(&depends) || has_misc_depends(&pre_depends) {
            continue;
        }

        // Get line number for package stanza (Depends field may not exist yet)
        let line_no = binary.as_deb822().line() + 1;

        let issue = LintianIssue {
            package: Some(package_name.clone()),
            package_type: Some(crate::PackageType::Binary),
            tag: Some("debhelper-but-no-misc-depends".to_string()),
            info: Some(format!(
                "(in section for {}) Depends [debian/control:{}]",
                package_name, line_no
            )),
        };

        if issue.should_fix(base_path) {
            // Add ${misc:Depends} to Depends using Relations API
            let mut relations: Relations = if depends.trim().is_empty() {
                Relations::new()
            } else {
                let (relations, _errors) = Relations::parse_relaxed(&depends, true);
                relations
            };

            relations.ensure_substvar("${misc:Depends}").unwrap();
            binary.set_depends(Some(&relations));
            misc_depends_added.push(package_name);
            fixed_issues.push(issue);
        } else {
            overridden_issues.push(issue);
        }
    }

    if misc_depends_added.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    let description = format!(
        "Add missing ${{misc:Depends}} to Depends for {}.",
        misc_depends_added.join(", ")
    );

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "debhelper-but-no-misc-depends",
    tags: ["debhelper-but-no-misc-depends"],
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
    fn test_add_misc_depends() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Build-Depends: debhelper (>= 9)

Package: test-package
Architecture: any
Depends: ${shlibs:Depends}
Description: Test package
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("${misc:Depends}"));
        assert!(updated_content.contains("${shlibs:Depends}"));
    }

    #[test]
    fn test_already_has_misc_depends() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Build-Depends: debhelper (>= 9)

Package: test-package
Architecture: any
Depends: ${shlibs:Depends}, ${misc:Depends}
Description: Test package
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_has_in_pre_depends() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Build-Depends: debhelper (>= 9)

Package: test-package
Architecture: any
Pre-Depends: ${misc:Depends}
Depends: ${shlibs:Depends}
Description: Test package
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debhelper() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Build-Depends: build-essential

Package: test-package
Architecture: any
Depends: ${shlibs:Depends}
Description: Test package
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_empty_depends() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Build-Depends: debhelper-compat (= 13)

Package: test-package
Architecture: any
Description: Test package
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Depends: ${misc:Depends}"));
    }

    #[test]
    fn test_skip_for_compat_14() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Build-Depends: debhelper-compat (= 14)

Package: test-package
Architecture: any
Depends: ${shlibs:Depends}
Description: Test package
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Verify the control file was not modified
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("${misc:Depends}"));
    }

    #[test]
    fn test_skip_for_compat_15() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Build-Depends: debhelper-compat (= 15)

Package: test-package
Architecture: any
Depends: ${shlibs:Depends}
Description: Test package
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Verify the control file was not modified
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("${misc:Depends}"));
    }

    #[test]
    fn test_still_works_for_compat_13() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Build-Depends: debhelper-compat (= 13)

Package: test-package
Architecture: any
Depends: ${shlibs:Depends}
Description: Test package
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("${misc:Depends}"));
        assert!(updated_content.contains("${shlibs:Depends}"));
    }
}
