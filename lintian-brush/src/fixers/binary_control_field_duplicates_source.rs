use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType, Visibility};
use debian_workspace::Workspace;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    let source_fields: BTreeMap<String, String> = source
        .as_deb822()
        .keys()
        .map(|k| {
            (
                k.to_string(),
                source.as_deb822().get(&k).unwrap_or_default(),
            )
        })
        .collect();

    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        let line_no = binary.as_deb822().line() + 1;
        let paragraph = binary.as_deb822();
        let Some(package_name) = binary.name() else {
            continue;
        };

        for key in paragraph.keys() {
            let Some(value) = paragraph.get(&key) else {
                continue;
            };
            let Some(source_value) = source_fields.get(key.as_str()) else {
                continue;
            };
            if source_value != &value {
                continue;
            }

            let issue = LintianIssue {
                package: Some(package_name.clone()),
                package_type: Some(PackageType::Binary),
                visibility: Some(Visibility::Info),
                tag: Some("installable-field-mirrors-source".to_string()),
                info: Some(format!("{} [debian/control:{}]", key, line_no)),
            };

            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!(
                    "Field {} on binary package {} duplicates source paragraph.",
                    key, package_name
                ),
                format!(
                    "Remove field {} from binary package {} that duplicates source.",
                    key, package_name
                ),
                vec![Action::Deb822(Deb822Action::RemoveField {
                    file: PathBuf::from("debian/control"),
                    paragraph: ParagraphSelector::Binary {
                        package: package_name.clone(),
                    },
                    field: key.to_string(),
                })],
            ));
        }
    }

    Ok(diagnostics)
}

/// Aggregate "field X on packages A, B" or, when several distinct fields
/// are involved, the bullet-list form the original fixer used.
fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    // field -> sorted unique package names that had it removed
    let mut by_field: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for action in actions {
        if let Action::Deb822(Deb822Action::RemoveField {
            paragraph: ParagraphSelector::Binary { package },
            field,
            ..
        }) = action
        {
            by_field
                .entry(field.as_str())
                .or_default()
                .push(package.as_str());
        }
    }
    for packages in by_field.values_mut() {
        packages.sort();
        packages.dedup();
    }

    if by_field.len() == 1 {
        let (field, packages) = by_field.iter().next().unwrap();
        format!(
            "Remove field {} on binary package{} {} that duplicates source.",
            field,
            if packages.len() != 1 { "s" } else { "" },
            packages.join(", ")
        )
    } else {
        let mut msg = String::from("Remove fields on binary packages that duplicate source.");
        for (field, packages) in &by_field {
            for package in packages {
                msg.push_str(&format!("\n+ Field {} from {}.", field, package));
            }
        }
        msg
    }
}

declare_detector! {
    name: "binary-control-field-duplicates-source",
    tags: ["installable-field-mirrors-source"],
    triggers: [
        // Any field in any binary paragraph may duplicate the source's
        // value; matching on `*` keeps the trigger broad.
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "*",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("blah".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_removes_duplicate_priority() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nSection: net\nPriority: optional\n\nPackage: blah\nSection: vcs\nPriority: optional\nDescription: test\n",
        )
        .unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove field Priority on binary package blah that duplicates source."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: blah\nSection: net\nPriority: optional\n\nPackage: blah\nSection: vcs\nDescription: test\n",
        );
    }

    #[test]
    fn test_removes_multiple_duplicate_fields() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nSection: net\nPriority: optional\n\nPackage: blah\nSection: net\nPriority: optional\nDescription: test\n",
        )
        .unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        // Two distinct fields removed -> bullet-list form.
        assert_eq!(
            result.description,
            "Remove fields on binary packages that duplicate source.\n+ Field Priority from blah.\n+ Field Section from blah.",
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: blah\nSection: net\nPriority: optional\n\nPackage: blah\nDescription: test\n",
        );
    }

    #[test]
    fn test_no_change_when_fields_differ() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: blah\nSection: net\nPriority: optional\n\nPackage: blah\nSection: vcs\nPriority: extra\nDescription: test\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_no_change_when_no_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
