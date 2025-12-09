use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::control::TemplatedControlEditor;
use debian_control::lossless::relations::Relations;
use std::path::Path;

fn has_misc_pre_depends(field_value: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(field_value, true);

    // Check if ${misc:Pre-Depends} substvar is present
    let substvars: Vec<String> = relations.substvars().collect();
    substvars.iter().any(|s| s == "${misc:Pre-Depends}")
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    // Check debhelper compat level
    let compat_version = debian_analyzer::debhelper::get_debhelper_compat_level(base_path)?;
    if let Some(version) = compat_version {
        if version <= 11 {
            // N/A for compat level <= 11
            return Err(FixerError::NoChanges);
        }
    } else {
        // No compat level found, N/A
        return Err(FixerError::NoChanges);
    }

    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut added = Vec::new();

    // Check each binary package
    for mut binary in editor.binaries() {
        let package_name = match binary.name() {
            Some(name) => name.to_string(),
            None => {
                log::debug!("Skipping binary package without name");
                continue;
            }
        };

        // Check if both init script and systemd/upstart unit exist
        let debian_dir = base_path.join("debian");
        let init_path = debian_dir.join(format!("{}.init", package_name));
        let service_path = debian_dir.join(format!("{}.service", package_name));
        let upstart_path = debian_dir.join(format!("{}.upstart", package_name));

        if !init_path.exists() {
            continue;
        }

        if !service_path.exists() && !upstart_path.exists() {
            continue;
        }

        // Check if ${misc:Pre-Depends} is already present
        let pre_depends = binary
            .pre_depends()
            .map(|s| s.to_string())
            .unwrap_or_default();

        if has_misc_pre_depends(&pre_depends) {
            continue;
        }

        // Add ${misc:Pre-Depends} to Pre-Depends
        let mut relations: Relations = if pre_depends.trim().is_empty() {
            Relations::new()
        } else {
            let (relations, _errors) = Relations::parse_relaxed(&pre_depends, true);
            relations
        };

        relations.ensure_substvar("${misc:Pre-Depends}").unwrap();
        binary.set_pre_depends(Some(&relations));
        added.push(package_name);
    }

    if added.is_empty() {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    let description = format!(
        "Add missing Pre-Depends: ${{misc:Pre-Depends}} in {}.",
        added.join(", ")
    );

    Ok(FixerResult::builder(&description)
        .fixed_tag("skip-systemd-native-flag-missing-pre-depends")
        .build())
}

declare_fixer! {
    name: "skip-systemd-native-flag-missing-pre-depends",
    tags: ["skip-systemd-native-flag-missing-pre-depends"],
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
    fn test_add_misc_pre_depends() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: blah
Build-Depends: debhelper-compat (= 12)

Package: blah
Description: description
 longer description
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        // Create debian/compat file for compat level 12
        fs::write(debian_dir.join("compat"), "12\n").unwrap();

        // Create both init and service files
        fs::write(debian_dir.join("blah.init"), "").unwrap();
        fs::write(debian_dir.join("blah.service"), "").unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(
            updated_content.contains("Pre-Depends: ${misc:Pre-Depends}"),
            "Expected Pre-Depends to contain ${{misc:Pre-Depends}}, got: {}",
            updated_content
        );
    }

    #[test]
    fn test_already_has_misc_pre_depends() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: blah
Build-Depends: debhelper-compat (= 12)

Package: blah
Pre-Depends: ${misc:Pre-Depends}
Description: description
 longer description
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        fs::write(debian_dir.join("compat"), "12\n").unwrap();
        fs::write(debian_dir.join("blah.init"), "").unwrap();
        fs::write(debian_dir.join("blah.service"), "").unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_init_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: blah
Build-Depends: debhelper-compat (= 12)

Package: blah
Description: description
 longer description
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        fs::write(debian_dir.join("compat"), "12\n").unwrap();
        // Only service file, no init file
        fs::write(debian_dir.join("blah.service"), "").unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_service_or_upstart_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: blah
Build-Depends: debhelper-compat (= 12)

Package: blah
Description: description
 longer description
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        fs::write(debian_dir.join("compat"), "12\n").unwrap();
        // Only init file, no service or upstart
        fs::write(debian_dir.join("blah.init"), "").unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_compat_level_too_old() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: blah
Build-Depends: debhelper-compat (= 11)

Package: blah
Description: description
 longer description
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        fs::write(debian_dir.join("compat"), "11\n").unwrap();
        fs::write(debian_dir.join("blah.init"), "").unwrap();
        fs::write(debian_dir.join("blah.service"), "").unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_upstart_instead_of_service() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: blah
Build-Depends: debhelper-compat (= 12)

Package: blah
Description: description
 longer description
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        fs::write(debian_dir.join("compat"), "12\n").unwrap();
        fs::write(debian_dir.join("blah.init"), "").unwrap();
        fs::write(debian_dir.join("blah.upstart"), "").unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Pre-Depends: ${misc:Pre-Depends}"));
    }
}
