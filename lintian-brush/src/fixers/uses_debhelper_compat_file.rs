use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_analyzer::debhelper::{highest_stable_compat_level, read_debhelper_compat_file};
use debian_analyzer::relations::is_relation_implied;
use debian_control::lossless::Entry;
use debversion::Version;
use std::path::Path;
use std::str::FromStr;

/// Check if the package uses CDBS by looking for cdbs include in debian/rules
fn check_cdbs(base_path: &Path) -> bool {
    let rules_path = base_path.join("debian/rules");
    if let Ok(content) = std::fs::read_to_string(&rules_path) {
        content.contains("/usr/share/cdbs/")
    } else {
        false
    }
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let compat_path = base_path.join("debian/compat");

    // Check if debian/compat exists
    let debhelper_compat_version = match read_debhelper_compat_file(&compat_path)? {
        Some(version) => version,
        None => return Err(FixerError::NoChanges),
    };

    // debhelper >= 11 supports the magic debhelper-compat build-dependency
    if debhelper_compat_version < 11 {
        return Err(FixerError::NoChanges);
    }

    // Exclude cdbs, since it only knows to get the debhelper compat version from debian/compat
    if check_cdbs(base_path) {
        return Err(FixerError::NoChanges);
    }

    // debhelper-compat is only supported for stable compat levels
    if debhelper_compat_version > highest_stable_compat_level() {
        return Err(FixerError::NoChanges);
    }

    let issue = LintianIssue::source_with_info(
        "uses-debhelper-compat-file",
        vec!["[debian/compat]".to_string()],
    );

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Parse and edit the control file
    let control_path = base_path.join("debian/control");
    let editor = TemplatedControlEditor::open(&control_path)?;

    let Some(mut source) = editor.source() else {
        return Err(FixerError::NoChanges);
    };

    // Create a target entry: debhelper (>= compat_version) for comparison
    let target_str = format!("debhelper (>= {})", debhelper_compat_version);
    let target_entry = Entry::from_str(&target_str)
        .map_err(|e| FixerError::Other(format!("Failed to parse target entry: {:?}", e)))?;

    // Process all three build dependency fields to remove debhelper
    for field in ["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"] {
        let Some(mut deps) = (match field {
            "Build-Depends" => source.build_depends(),
            "Build-Depends-Indep" => source.build_depends_indep(),
            "Build-Depends-Arch" => source.build_depends_arch(),
            _ => None,
        }) else {
            continue;
        };

        // Check if this field has a debhelper dependency
        if !deps.has_relation("debhelper") {
            continue;
        }

        let Ok((_pos, entry)) = deps.get_relation("debhelper") else {
            continue;
        };

        // Only remove if the entry is implied by debhelper >= compat_version
        if !is_relation_implied(&entry, &target_entry) {
            continue;
        }

        // Remove debhelper
        deps.drop_dependency("debhelper");

        // Update or remove the field
        if deps.is_empty() {
            source.as_mut_deb822().remove(field);
        } else {
            match field {
                "Build-Depends" => source.set_build_depends(&deps),
                _ => source.set(field, &deps.to_string()),
            }
        }
    }

    // Add debhelper-compat to Build-Depends (Relations will sort appropriately)
    let mut build_depends = source.build_depends().unwrap_or_default();

    // Create the debhelper-compat entry
    let compat_version_str = debhelper_compat_version.to_string();
    let version = Version::from_str(&compat_version_str)
        .map_err(|e| FixerError::Other(format!("Failed to parse version: {:?}", e)))?;

    debian_analyzer::relations::ensure_exact_version(
        &mut build_depends,
        "debhelper-compat",
        &version,
        None,
    );

    source.set_build_depends(&build_depends);

    // Commit the control file changes
    editor.commit()?;

    // Delete the debian/compat file
    std::fs::remove_file(&compat_path)?;

    Ok(
        FixerResult::builder("Set debhelper-compat version in Build-Depends.")
            .fixed_issue(issue)
            .build(),
    )
}

crate::declare_fixer! {
    name: "uses-debhelper-compat-file",
    tags: ["uses-debhelper-compat-file"],
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
    fn test_simple() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Create debian/compat
        let compat_path = debian_dir.join("compat");
        fs::write(&compat_path, "11\n").unwrap();

        // Create debian/control
        let control_content = r#"Source: f2fs-tools
Section: admin
Priority: optional
Maintainer: Jelmer Vernooĳ <jelmer@debian.org>
Build-Depends:
 debhelper (>= 11),
 pkg-config,
 uuid-dev
Standards-Version: 4.2.0

Package: blah
Architecture: linux-any
Description: test
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "f2fs-tools", &version, &Default::default());
        assert!(result.is_ok());

        // Check that compat file is deleted
        assert!(!compat_path.exists());

        // Check that control file is updated
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("debhelper-compat (= 11)"));
        assert!(!updated_content.contains("debhelper (>= 11)"));
    }

    #[test]
    fn test_no_change_when_compat_too_old() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Create debian/compat with old version
        let compat_path = debian_dir.join("compat");
        fs::write(&compat_path, "9\n").unwrap();

        // Create debian/control
        let control_content = r#"Source: test
Build-Depends: debhelper (>= 9)

Package: test
Description: test
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Compat file should still exist
        assert!(compat_path.exists());
    }
}
