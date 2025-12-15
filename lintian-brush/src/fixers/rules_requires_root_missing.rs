use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;
use std::str::FromStr;

fn is_debcargo_package(base_path: &Path) -> bool {
    base_path.join("debian/debcargo.toml").exists()
}

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    if is_debcargo_package(base_path) {
        return Err(FixerError::NoChanges);
    }

    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    let mut source = editor.source().ok_or(FixerError::NoChanges)?;
    let current_require_root = source.as_deb822().get("Rules-Requires-Root");

    // Get the compat_release from preferences, defaulting to "sid"
    let compat_release = preferences.compat_release.as_deref().unwrap_or("sid");

    // Get the oldest dpkg version for the compat release
    let oldest_dpkg_version = debian_analyzer::release_info::dpkg_versions
        .get(compat_release)
        .cloned();

    let dpkg_1_22_13 = debversion::Version::from_str("1.22.13").unwrap();

    if current_require_root.is_none() {
        // No Rules-Requires-Root field exists
        if let Some(ref dpkg_version) = oldest_dpkg_version {
            if dpkg_version < &dpkg_1_22_13 {
                // Only add the field if dpkg < 1.22.13
                let issue = LintianIssue::source_with_info(
                    "silent-on-rules-requiring-root",
                    vec!["[debian/control]".to_string()],
                );

                if !issue.should_fix(base_path) {
                    return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
                }

                // TODO: add some heuristics to set require_root = "yes" in common
                // cases, like `debian/rules binary` chown(1)'ing stuff
                source.set_rules_requires_root(false);
                editor.commit()?;

                return Ok(FixerResult::builder("Set Rules-Requires-Root: no.")
                    .fixed_issue(issue)
                    .certainty(crate::Certainty::Possible)
                    .build());
            }
        }
    } else if current_require_root.as_deref() == Some("no") {
        // The default value is "no" as of dpkg 1.22.13
        // If the oldest support version of dpkg is >= 1.22.13, we can assume
        // that the field can be unset if it is "no".
        if let Some(ref dpkg_version) = oldest_dpkg_version {
            if dpkg_version >= &dpkg_1_22_13 {
                source.as_mut_deb822().remove("Rules-Requires-Root");
                editor.commit()?;

                return Ok(FixerResult::builder("Removed Rules-Requires-Root")
                    .certainty(crate::Certainty::Possible)
                    .build());
            }
        }
    }

    Err(FixerError::NoChanges)
}

declare_fixer! {
    name: "rules-requires-root-missing",
    tags: ["silent-on-rules-requiring-root"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let preferences = FixerPreferences::default();

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_debcargo_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create debcargo.toml to indicate this is a debcargo package
        fs::write(debian_dir.join("debcargo.toml"), "").unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\n\nPackage: test-package\n",
        )
        .unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_add_rules_requires_root_old_dpkg() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nMaintainer: Test <test@example.com>\n\nPackage: test-package\nArchitecture: all\nDescription: Test package\n",
        )
        .unwrap();

        let mut preferences = FixerPreferences::default();
        preferences.compat_release = Some("bullseye".to_string());

        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.description, "Set Rules-Requires-Root: no.");

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Rules-Requires-Root: no"));
    }

    #[test]
    fn test_no_change_new_dpkg() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nMaintainer: Test <test@example.com>\n\nPackage: test-package\nArchitecture: all\nDescription: Test package\n",
        )
        .unwrap();

        let mut preferences = FixerPreferences::default();
        preferences.compat_release = Some("trixie".to_string());

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("Rules-Requires-Root"));
    }

    #[test]
    fn test_remove_rules_requires_root_new_dpkg() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nRules-Requires-Root: no\nMaintainer: Test <test@example.com>\n\nPackage: test-package\nArchitecture: all\nDescription: Test package\n",
        )
        .unwrap();

        let mut preferences = FixerPreferences::default();
        preferences.compat_release = Some("trixie".to_string());

        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.description, "Removed Rules-Requires-Root");

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("Rules-Requires-Root"));
    }

    #[test]
    fn test_no_remove_rules_requires_root_yes() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nRules-Requires-Root: yes\nMaintainer: Test <test@example.com>\n\nPackage: test-package\nArchitecture: all\nDescription: Test package\n",
        )
        .unwrap();

        let mut preferences = FixerPreferences::default();
        preferences.compat_release = Some("trixie".to_string());

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Rules-Requires-Root: yes"));
    }
}
