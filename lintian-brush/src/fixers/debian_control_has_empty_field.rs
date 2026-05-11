use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// Per-diagnostic message tag separator. The describer parses these back out
/// to assemble the aggregate description.
const SEP: char = '\t';

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut diagnostics = Vec::new();

    if let Some(source) = control.source() {
        let paragraph = source.as_deb822();
        for entry in paragraph.entries() {
            if !entry.value().trim().is_empty() {
                continue;
            }
            let Some(key) = entry.key() else {
                continue;
            };
            let line_number = entry.line() + 1;
            let issue = LintianIssue::source_with_info(
                "debian-control-has-empty-field",
                Visibility::Info,
                vec![format!(
                    "(in source paragraph) {} [debian/control:{}]",
                    key, line_number
                )],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!("source{}{}", SEP, key),
                format!("Remove empty field {} from source paragraph.", key),
                vec![Action::Deb822(Deb822Action::RemoveField {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Source,
                    field: key.to_string(),
                })],
            ));
        }
    }

    for binary in control.binaries() {
        let paragraph = binary.as_deb822();
        let Some(package_name) = paragraph.get("Package") else {
            continue;
        };
        for entry in paragraph.entries() {
            if !entry.value().trim().is_empty() {
                continue;
            }
            let Some(key) = entry.key() else {
                continue;
            };
            let line_number = entry.line() + 1;
            let issue = LintianIssue {
                package: Some(package_name.clone()),
                package_type: Some(PackageType::Binary),
                visibility: Some(Visibility::Info),
                tag: Some("debian-control-has-empty-field".to_string()),
                info: Some(format!(
                    "(in section for {}) {} [debian/control:{}]",
                    package_name, key, line_number
                )),
            };
            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!("{}{}{}", package_name, SEP, key),
                format!(
                    "Remove empty field {} from binary package {}.",
                    key, package_name
                ),
                vec![Action::Deb822(Deb822Action::RemoveField {
                    file: control_rel.clone(),
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

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let mut fields: Vec<String> = Vec::new();
    let mut packages: Vec<String> = Vec::new();
    for (d, _) in fixed {
        let Some((scope, field)) = d.message.split_once(SEP) else {
            continue;
        };
        fields.push(field.to_string());
        if scope != "source" {
            packages.push(scope.to_string());
        }
    }
    fields.dedup();
    packages.sort();
    packages.dedup();

    let field_text = if fields.len() == 1 { "field" } else { "fields" };
    let package_text = if packages.is_empty() {
        String::new()
    } else {
        format!(" in package {}", packages.join(", "))
    };

    format!(
        "debian/control: Remove empty control {} {}{}.",
        field_text,
        fields.join(", "),
        package_text
    )
}

declare_detector! {
    name: "debian-control-has-empty-field",
    tags: ["debian-control-has-empty-field"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "*",
        },
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
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_empty_fields() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test-package\nDepends:\n\nPackage: test-package\nDescription: Test package\n Description text\nProvides:\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-package\n\nPackage: test-package\nDescription: Test package\n Description text\n",
        );
    }

    #[test]
    fn test_no_empty_fields() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nMaintainer: Test <test@example.com>\n\nPackage: test-package\nDescription: Test package\n Description text\nDepends: libc6\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_whitespace_only_fields() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test-package\nBuild-Depends:   \n\nPackage: test-package\nDescription: Test package\n Description text\nProvides:  \t\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-package\n\nPackage: test-package\nDescription: Test package\n Description text\n",
        );
    }
}
