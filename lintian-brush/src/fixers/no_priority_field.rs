use crate::{FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    // Get the compat_release from preferences, defaulting to "sid"
    let compat_release = preferences.compat_release.as_deref().unwrap_or("sid");

    // Get the oldest dpkg version for the compat release
    let oldest_dpkg_version = debian_analyzer::release_info::dpkg_versions
        .get(compat_release)
        .cloned();

    let dpkg_1_22_13 = debversion::Version::from_str("1.22.13").unwrap();

    // Check if we're targeting dpkg >= 1.22.13
    let default_priority_is_optional = if let Some(ref dpkg_version) = oldest_dpkg_version {
        dpkg_version >= &dpkg_1_22_13
    } else {
        // For sid/unstable, assume the latest behavior
        true
    };

    // If source already has Priority, we might want to remove it if it's "optional" and dpkg >= 1.22.13
    if let Some(source) = editor.source() {
        if let Some(priority) = source.as_deb822().get("Priority") {
            if priority == "optional" && default_priority_is_optional {
                // Remove redundant Priority: optional from source stanza
                let mut source = editor.source().unwrap();
                source.as_mut_deb822().remove("Priority");
                editor.commit()?;
                return Ok(FixerResult::builder(
                    "Remove Priority: optional from source stanza (it is now the default).",
                )
                .certainty(crate::Certainty::Confident)
                .build());
            }
            return Err(FixerError::NoChanges);
        }
    }

    let mut binary_priorities = HashSet::new();
    let mut updated = HashMap::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Collect binaries to process
    let binaries: Vec<_> = editor.binaries().collect();

    for mut binary in binaries {
        let paragraph = binary.as_mut_deb822();
        let package_name = paragraph.get("Package").unwrap_or_default().to_string();

        if let Some(priority) = paragraph.get("Priority") {
            binary_priorities.insert(priority.to_string());
        } else {
            // Only add Priority: optional if dpkg < 1.22.13
            if !default_priority_is_optional {
                // Create issue for missing priority field
                let issue = LintianIssue::source_with_info(
                    "recommended-field",
                    vec![format!("debian/control Priority")],
                );
                if issue.should_fix(base_path) {
                    // Set priority to "optional" for binaries without it
                    paragraph.set("Priority", "optional");
                    binary_priorities.insert("optional".to_string());
                    updated.insert(package_name, "optional".to_string());
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
            } else {
                // Priority: optional is the default, so we don't need to add it
                binary_priorities.insert("optional".to_string());
            }
        }
    }

    // If all issues were overridden, return NoChangesAfterOverrides
    if fixed_issues.is_empty() && !overridden_issues.is_empty() {
        return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
    }

    // If all binaries have the same priority, move it to source (only if it's not the default)
    if binary_priorities.len() == 1 {
        let common_priority = binary_priorities.iter().next().unwrap().clone();

        // Check if any binary actually has Priority set explicitly
        let binaries: Vec<_> = editor.binaries().collect();
        let any_explicit = binaries
            .iter()
            .any(|b| b.as_deb822().get("Priority").is_some());

        // Only set Priority in source if:
        // 1. At least one binary has it set explicitly
        // 2. Either the priority is not "optional" OR dpkg < 1.22.13
        if any_explicit && (common_priority != "optional" || !default_priority_is_optional) {
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

            // Add fixed and overridden issues
            if !fixed_issues.is_empty() {
                result_builder = result_builder.fixed_issues(fixed_issues);
            }
            if !overridden_issues.is_empty() {
                result_builder = result_builder.overridden_issues(overridden_issues);
            }

            return Ok(result_builder.build());
        }
    }

    if !updated.is_empty() {
        editor.commit()?;

        let packages_str: Vec<String> = updated
            .iter()
            .map(|(pkg, prio)| format!("{} ({})", pkg, prio))
            .collect();

        return Ok(FixerResult::builder(format!(
            "Set priority for binary packages {:?}.",
            packages_str
        ))
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build());
    }

    Err(FixerError::NoChanges)
}

declare_fixer! {
    name: "no-priority-field",
    tags: ["recommended-field"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_missing_priority_old_dpkg() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: foo\n\nPackage: blah\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("bullseye".to_string());
        let result = fixer.apply(temp_dir.path(), "foo", &version, &preferences);
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            updated_content,
            "Source: foo\nPriority: optional\n\nPackage: blah\n"
        );
    }

    #[test]
    fn test_missing_priority_new_dpkg() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: foo\n\nPackage: blah\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("trixie".to_string());
        let result = fixer.apply(temp_dir.path(), "foo", &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(updated_content, "Source: foo\n\nPackage: blah\n");
    }

    #[test]
    fn test_common_priority_old_dpkg() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content =
            "Source: foo\n\nPackage: foo\nPriority: optional\n\nPackage: foo-doc\nPriority: optional\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("bullseye".to_string());
        let result = fixer.apply(temp_dir.path(), "foo", &version, &preferences);
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            updated_content,
            "Source: foo\nPriority: optional\n\nPackage: foo\n\nPackage: foo-doc\n"
        );
    }

    #[test]
    fn test_common_priority_new_dpkg() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content =
            "Source: foo\n\nPackage: foo\nPriority: optional\n\nPackage: foo-doc\nPriority: optional\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("trixie".to_string());
        let result = fixer.apply(temp_dir.path(), "foo", &version, &preferences);
        // With dpkg >= 1.22.13, Priority: optional in binaries doesn't need to be moved to source
        assert!(matches!(result, Err(FixerError::NoChanges)));

        let updated_content = fs::read_to_string(&control_path).unwrap();
        // The Priority fields should remain unchanged
        assert_eq!(updated_content, "Source: foo\n\nPackage: foo\nPriority: optional\n\nPackage: foo-doc\nPriority: optional\n");
    }

    #[test]
    fn test_remove_redundant_priority_from_source() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: foo\nPriority: optional\n\nPackage: foo\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("trixie".to_string());
        let result = fixer.apply(temp_dir.path(), "foo", &version, &preferences);
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(updated_content, "Source: foo\n\nPackage: foo\n");
    }

    #[test]
    fn test_already_set_in_source_old_dpkg() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: foo\nPriority: optional\n\nPackage: foo\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("bullseye".to_string());
        let result = fixer.apply(temp_dir.path(), "foo", &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_set_in_source_non_optional() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: foo\nPriority: important\n\nPackage: foo\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("trixie".to_string());
        let result = fixer.apply(temp_dir.path(), "foo", &version, &preferences);
        // Priority: important should not be removed even with new dpkg
        assert!(matches!(result, Err(FixerError::NoChanges)));

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            updated_content,
            "Source: foo\nPriority: important\n\nPackage: foo\n"
        );
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
