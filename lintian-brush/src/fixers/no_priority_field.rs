use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;

pub fn detect(
    ws: &dyn Workspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let editor = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };

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
                let issue = LintianIssue::source_with_info(
                    "redundant-field",
                    Visibility::Info,
                    vec!["debian/control Source Priority".to_string()],
                );
                let plans = vec![ActionPlan {
                    label: "Remove redundant Priority: optional from source stanza.".to_string(),
                    opinionated: false,
                    certainty: None,
                    actions: vec![Action::Deb822(Deb822Action::RemoveField {
                        file: control_rel.clone(),
                        paragraph: ParagraphSelector::Source,
                        field: "Priority".to_string(),
                    })],
                }];
                let diagnostic = Diagnostic {
                    issue: Some(issue),
                    message:
                        "Priority: optional in source stanza is redundant with dpkg >= 1.22.13 and can be removed."
                            .to_string(),
                    plans,
                    certainty: Some(crate::Certainty::Confident),
                    patch_name: None,
                };
                return Ok(vec![diagnostic]);
            }
            return Ok(Vec::new());
        }
    }

    let mut binary_priorities = HashSet::new();
    let mut missing_priorities = Vec::new();
    let mut any_explicit = false;

    // Collect binaries to process
    let binaries: Vec<_> = editor.binaries().collect();

    for binary in &binaries {
        let paragraph = binary.as_deb822();
        let package_name = paragraph.get("Package").unwrap_or_default().to_string();

        if let Some(priority) = paragraph.get("Priority") {
            binary_priorities.insert(priority.to_string());
            any_explicit = true;
        } else {
            missing_priorities.push(package_name);
            binary_priorities.insert("optional".to_string());
            // Since it's missing, it's not explicit yet, but we're implicitly adding "optional".
        }
    }

    let mut diagnostics = Vec::new();

    // If all binaries have the same priority, move it to source (only if it's not the default)
    if binary_priorities.len() == 1 {
        let common_priority = binary_priorities.iter().next().unwrap().clone();

        // If we added it implicitly to all missing ones, it's as if they were explicit
        // if we are going to write it to source anyway.
        // But wait! If none had it explicitly, we only write to source if we actually needed to add it (i.e. !default_priority_is_optional).
        if any_explicit || !default_priority_is_optional {
            if common_priority != "optional" || !default_priority_is_optional {
                let mut actions = vec![Action::Deb822(Deb822Action::SetField {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Source,
                    field: "Priority".to_string(),
                    value: common_priority.clone(),
                })];

                for binary in &binaries {
                    let package_name = binary
                        .as_deb822()
                        .get("Package")
                        .unwrap_or_default()
                        .to_string();
                    if binary.as_deb822().get("Priority").is_some() {
                        actions.push(Action::Deb822(Deb822Action::RemoveField {
                            file: control_rel.clone(),
                            paragraph: ParagraphSelector::Binary {
                                package: package_name,
                            },
                            field: "Priority".to_string(),
                        }));
                    }
                }

                let issue = if !missing_priorities.is_empty() {
                    Some(LintianIssue::source_with_info(
                        "recommended-field",
                        Visibility::Warning,
                        vec![format!("debian/control Priority")],
                    ))
                } else {
                    None
                };

                let plans = vec![ActionPlan {
                    label: "Set priority in source stanza, since it is the same for all packages."
                        .to_string(),
                    opinionated: false,
                    certainty: None,
                    actions,
                }];

                diagnostics.push(Diagnostic {
                    issue,
                    message:
                        "Set priority in source stanza, since it is the same for all packages."
                            .to_string(),
                    plans,
                    certainty: Some(crate::Certainty::Confident),
                    patch_name: None,
                });
            }
        }
    }

    if diagnostics.is_empty() && !default_priority_is_optional {
        // We couldn't move it to source, so we must add it to the binaries that are missing it
        for package_name in missing_priorities {
            let issue = LintianIssue::source_with_info(
                "recommended-field",
                Visibility::Warning,
                vec!["debian/control Priority".to_string()],
            );
            let plans = vec![ActionPlan {
                label: "Set priority to 'optional' for this binary package.".to_string(),
                opinionated: false,
                certainty: None,
                actions: vec![Action::Deb822(Deb822Action::SetField {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: package_name.clone(),
                    },
                    field: "Priority".to_string(),
                    value: "optional".to_string(),
                })],
            }];
            diagnostics.push(Diagnostic {
                issue: Some(issue),
                message: format!(
                    "Binary package '{}' is missing a Priority field.",
                    package_name
                ),
                plans,
                certainty: None,
                patch_name: None,
            });
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "no-priority-field",
    tags: ["recommended-field"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Priority",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Priority",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use std::fs;
    use tempfile::TempDir;

    fn make_fixer() -> DetectorImpl {
        DetectorImpl
    }

    #[test]
    fn test_missing_priority_old_dpkg() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: foo\n\nPackage: blah\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = make_fixer();
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("bullseye".to_string());
        let result = {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                temp_dir.path(),
                Some("foo".into()),
                Some(version.clone()),
            );
            fixer.apply(&ws, &preferences)
        };
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

        let fixer = make_fixer();
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("trixie".to_string());
        let result = {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                temp_dir.path(),
                Some("foo".into()),
                Some(version.clone()),
            );
            fixer.apply(&ws, &preferences)
        };
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

        let fixer = make_fixer();
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("bullseye".to_string());
        let result = {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                temp_dir.path(),
                Some("foo".into()),
                Some(version.clone()),
            );
            fixer.apply(&ws, &preferences)
        };
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

        let fixer = make_fixer();
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("trixie".to_string());
        let result = {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                temp_dir.path(),
                Some("foo".into()),
                Some(version.clone()),
            );
            fixer.apply(&ws, &preferences)
        };
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

        let fixer = make_fixer();
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("trixie".to_string());
        let result = {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                temp_dir.path(),
                Some("foo".into()),
                Some(version.clone()),
            );
            fixer.apply(&ws, &preferences)
        };
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

        let fixer = make_fixer();
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("bullseye".to_string());
        let result = {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                temp_dir.path(),
                Some("foo".into()),
                Some(version.clone()),
            );
            fixer.apply(&ws, &preferences)
        };
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

        let fixer = make_fixer();
        let version: crate::Version = "1.0".parse().unwrap();
        let mut preferences = crate::FixerPreferences::default();
        preferences.compat_release = Some("trixie".to_string());
        let result = {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                temp_dir.path(),
                Some("foo".into()),
                Some(version.clone()),
            );
            fixer.apply(&ws, &preferences)
        };
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

        let fixer = make_fixer();
        let version: crate::Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            temp_dir.path(),
            Some("test-package".into()),
            Some(version.clone()),
        );
        let result = fixer.apply(&ws, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
