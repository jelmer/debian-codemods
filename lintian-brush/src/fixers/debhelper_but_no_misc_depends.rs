use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::control::TemplatedControlEditor;
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

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut misc_depends_added = Vec::new();

    // Check if the source uses debhelper
    let uses_dh = if let Some(source) = editor.source() {
        let build_depends = source.as_deb822().get("Build-Depends").unwrap_or_default();
        uses_debhelper(&build_depends)
    } else {
        false
    };

    if !uses_dh {
        return Err(FixerError::NoChanges);
    }

    // Check each binary package
    for mut binary in editor.binaries() {
        let paragraph = binary.as_mut_deb822();
        let package_name = paragraph
            .get("Package")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let depends = paragraph.get("Depends").unwrap_or_default();
        let pre_depends = paragraph.get("Pre-Depends").unwrap_or_default();

        // Skip if already has ${misc:Depends} in either Depends or Pre-Depends
        if has_misc_depends(&depends) || has_misc_depends(&pre_depends) {
            continue;
        }

        // Add ${misc:Depends} to Depends using Relations API
        let mut relations: Relations = if depends.trim().is_empty() {
            Relations::new()
        } else {
            let (relations, _errors) = Relations::parse_relaxed(&depends, true);
            relations
        };

        relations.ensure_substvar("${misc:Depends}").unwrap();
        paragraph.set("Depends", &relations.to_string());
        misc_depends_added.push(package_name);
    }

    if misc_depends_added.is_empty() {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    let description = format!(
        "Add missing ${{misc:Depends}} to Depends for {}.",
        misc_depends_added.join(", ")
    );

    Ok(FixerResult::builder(&description)
        .fixed_tag("debhelper-but-no-misc-depends")
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
}
