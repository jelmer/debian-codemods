use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::control::TemplatedControlEditor;
use debversion::Version;
use std::path::Path;

const MINIMUM_DEBHELPER_VERSION: &str = "9.20160709";

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let mut editor = TemplatedControlEditor::open(&control_path)?;
    let mut made_changes = false;

    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        // Check each Build-Depends field type
        for field_name in ["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"] {
            if let Some(field_value) = paragraph.get(field_name) {
                use debian_control::lossless::relations::Relations;
                let (mut relations, _errors) = Relations::parse_relaxed(&field_value, true);

                if relations.drop_dependency("dh-systemd") {
                    paragraph.set(field_name, &relations.to_string());
                    made_changes = true;
                }
            }
        }

        if made_changes {
            // Ensure minimum debhelper version
            let build_depends_str = paragraph
                .get("Build-Depends")
                .unwrap_or_else(|| String::new());
            use debian_control::lossless::relations::Relations;
            let (mut build_depends, _errors) = Relations::parse_relaxed(&build_depends_str, true);

            let minimum_version: Version = MINIMUM_DEBHELPER_VERSION.parse().unwrap();
            build_depends.ensure_minimum_version("debhelper", &minimum_version);

            paragraph.set("Build-Depends", &build_depends.to_string());
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    Ok(
        FixerResult::builder("Depend on newer debhelper (>= 9.20160709) rather than dh-systemd.")
            .fixed_tag("build-depends-on-obsolete-package")
            .build(),
    )
}

declare_fixer! {
    name: "build-depends-on-obsolete-package",
    tags: ["build-depends-on-obsolete-package"],
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
    fn test_remove_dh_systemd() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: mypackage
Build-Depends: debhelper (>= 9), dh-systemd

Package: mypackage
Architecture: any
"#;

        fs::write(debian_dir.join("control"), control_content).unwrap();

        let result = run(base_path);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_control = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(!updated_control.contains("dh-systemd"));
        assert!(updated_control.contains("debhelper (>= 9.20160709)"));
    }

    #[test]
    fn test_no_dh_systemd() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: mypackage
Build-Depends: debhelper (>= 9)

Package: mypackage
Architecture: any
"#;

        fs::write(debian_dir.join("control"), control_content).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_remove_from_build_depends_indep() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_content = r#"Source: mypackage
Build-Depends: debhelper (>= 9)
Build-Depends-Indep: dh-systemd

Package: mypackage
Architecture: any
"#;

        fs::write(debian_dir.join("control"), control_content).unwrap();

        let result = run(base_path);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_control = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(!updated_control.contains("dh-systemd"));
        assert!(updated_control.contains("debhelper (>= 9.20160709)"));
    }
}
