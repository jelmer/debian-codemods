use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::collections::HashSet;
use std::path::PathBuf;

// Include the generated field definitions
include!(concat!(env!("OUT_DIR"), "/debian_control_fields.rs"));

/// Get the current vendor (e.g., "debian", "ubuntu")
fn get_vendor() -> String {
    std::process::Command::new("dpkg-vendor")
        .arg("--query")
        .arg("vendor")
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "debian".to_string())
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let vendor = get_vendor();
    let mut valid_field_names: HashSet<&'static str> = HashSet::new();
    valid_field_names.extend(known_debian_source_fields(&vendor));
    valid_field_names.extend(known_debian_binary_fields(&vendor));

    let mut diagnostics = Vec::new();

    if let Some(source) = control.source() {
        let paragraph = source.as_deb822();
        let keys: Vec<String> = paragraph.keys().map(|k| k.to_string()).collect();
        for field in keys {
            if valid_field_names.contains(field.as_str()) {
                continue;
            }
            for &valid_field in &valid_field_names {
                if valid_field.eq_ignore_ascii_case(&field) {
                    let issue = LintianIssue::source_with_info(
                        "cute-field",
                        Visibility::Pedantic,
                        vec![format!("{} vs {}", field, valid_field)],
                    );
                    diagnostics.push(Diagnostic::with_actions(
                        issue,
                        format!(
                            "Field name {} has wrong case (should be {}).",
                            field, valid_field
                        ),
                        format!("{} ⇒ {}", field, valid_field),
                        vec![Action::Deb822(Deb822Action::RenameField {
                            file: PathBuf::from("debian/control"),
                            paragraph: ParagraphSelector::Source,
                            from: field.clone(),
                            to: valid_field.to_string(),
                        })],
                    ));
                    break;
                }
            }
        }
    }

    for binary in control.binaries() {
        let Some(package_name) = binary.name() else {
            continue;
        };
        let paragraph = binary.as_deb822();
        let keys: Vec<String> = paragraph.keys().map(|k| k.to_string()).collect();
        for field in keys {
            if valid_field_names.contains(field.as_str()) {
                continue;
            }
            for &valid_field in &valid_field_names {
                if valid_field.eq_ignore_ascii_case(&field) {
                    let issue = LintianIssue::binary_with_info(
                        &package_name,
                        "cute-field",
                        Visibility::Pedantic,
                        vec![format!("{} vs {}", field, valid_field)],
                    );
                    diagnostics.push(Diagnostic::with_actions(
                        issue,
                        format!(
                            "Field name {} has wrong case (should be {}).",
                            field, valid_field
                        ),
                        format!("{} ⇒ {}", field, valid_field),
                        vec![Action::Deb822(Deb822Action::RenameField {
                            file: PathBuf::from("debian/control"),
                            paragraph: ParagraphSelector::Binary {
                                package: package_name.clone(),
                            },
                            from: field.clone(),
                            to: valid_field.to_string(),
                        })],
                    ));
                    break;
                }
            }
        }
    }

    Ok(diagnostics)
}

/// Aggregate per-rename diagnostics into one
/// "Fix field name case(s) in debian/control (X ⇒ Y, A ⇒ B)." line.
fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut pairs: Vec<(String, String)> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Deb822(Deb822Action::RenameField { from, to, .. }) => {
                Some((from.clone(), to.clone()))
            }
            _ => None,
        })
        .collect();
    pairs.sort();
    pairs.dedup();

    let kind = if pairs.len() > 1 { "cases" } else { "case" };
    let fixed_str = pairs
        .iter()
        .map(|(from, to)| format!("{} ⇒ {}", from, to))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Fix field name {} in debian/control ({}).", kind, fixed_str)
}

declare_detector! {
    name: "field-name-typo-in-control",
    tags: ["cute-field"],
    // Must fix field name typos before validating field content
    before: ["out-of-date-standards-version"],
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
    cost: crate::detector::DetectorCost::Subprocess,
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
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_fix_homepage_case() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nHomePage: https://www.example.com/\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Fix field name case in debian/control (HomePage ⇒ Homepage)."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: blah\nHomepage: https://www.example.com/\n",
        );
    }

    #[test]
    fn test_no_typos() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: blah\nHomepage: https://www.example.com/\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_multiple_typos() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nHomePage: https://www.example.com/\nmaintainer: John Doe <john@example.com>\n\nPackage: test\narchitecture: any\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Fix field name cases in debian/control (HomePage ⇒ Homepage, architecture ⇒ Architecture, maintainer ⇒ Maintainer).",
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: blah\nHomepage: https://www.example.com/\nMaintainer: John Doe <john@example.com>\n\nPackage: test\nArchitecture: any\n",
        );
    }
}
