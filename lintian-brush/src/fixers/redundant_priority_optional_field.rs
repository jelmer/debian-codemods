use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
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

    let source_priority = control.source().as_ref().and_then(|s| s.get("Priority"));

    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    if source_priority.as_deref() == Some("optional") {
        diagnostics.push(Diagnostic::with_actions(
            LintianIssue::source("redundant-priority-optional-field", Visibility::Pedantic),
            "Source stanza sets Priority: optional, which is the default.",
            "Remove redundant Priority: optional from source stanza.",
            vec![Action::Deb822(Deb822Action::RemoveField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Source,
                field: "Priority".into(),
            })],
        ));
    }

    // A binary's Priority: optional is only "redundant" against a default of
    // optional. If the source declares a non-optional Priority, the binary
    // override is meaningful and lintian does not emit the tag.
    let source_is_optional_or_unset =
        source_priority.is_none() || source_priority.as_deref() == Some("optional");

    if source_is_optional_or_unset {
        for binary in control.binaries() {
            if binary.get("Priority").as_deref() != Some("optional") {
                continue;
            }
            let Some(package_name) = binary.name() else {
                continue;
            };
            diagnostics.push(Diagnostic::with_actions(
                LintianIssue::binary_with_info(
                    &package_name,
                    "redundant-priority-optional-field",
                    Visibility::Pedantic,
                    vec![],
                ),
                format!(
                    "Binary stanza {} sets Priority: optional, which is the default.",
                    package_name
                ),
                format!(
                    "Remove redundant Priority: optional from binary stanza {}.",
                    package_name
                ),
                vec![Action::Deb822(Deb822Action::RemoveField {
                    file: PathBuf::from("debian/control"),
                    paragraph: ParagraphSelector::Binary {
                        package: package_name.clone(),
                    },
                    field: "Priority".into(),
                })],
            ));
        }
    }

    Ok(diagnostics)
}

/// Aggregate the per-stanza diagnostics into a single human-friendly line.
fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut source = false;
    let mut binaries: Vec<&str> = Vec::new();
    for action in actions {
        if let Action::Deb822(Deb822Action::RemoveField { paragraph, .. }) = action {
            match paragraph {
                ParagraphSelector::Source => source = true,
                ParagraphSelector::Binary { package } => binaries.push(package.as_str()),
                _ => {}
            }
        }
    }
    binaries.sort();
    binaries.dedup();

    match (source, binaries.as_slice()) {
        (true, []) => "Remove redundant Priority: optional from source stanza.".to_string(),
        (false, [pkg]) => format!(
            "Remove redundant Priority: optional from binary stanza {}.",
            pkg
        ),
        (false, pkgs) => format!(
            "Remove redundant Priority: optional from binary stanzas {}.",
            pkgs.join(", ")
        ),
        (true, pkgs) => format!(
            "Remove redundant Priority: optional from source and binary stanzas {}.",
            pkgs.join(", ")
        ),
    }
}

declare_detector! {
    name: "redundant-priority-optional-field",
    tags: ["redundant-priority-optional-field"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Priority",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Priority",
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
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, "test", "1.0".parse().unwrap());
        detect(&ws, &FixerPreferences::default())
    }

    fn write_control(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), content).unwrap();
    }

    #[test]
    fn test_removes_priority_from_source() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nPriority: optional\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Remove redundant Priority: optional from source stanza."
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_removes_priority_from_binary_when_source_unset() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nPriority: optional\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Remove redundant Priority: optional from binary stanza foo."
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_removes_priority_from_both_source_and_binary() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nPriority: optional\n\nPackage: foo\nPriority: optional\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Remove redundant Priority: optional from source and binary stanzas foo."
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_keeps_binary_priority_when_source_non_optional() {
        // Source declares Priority: standard, so the binary's Priority: optional
        // is a meaningful override — lintian does not emit the tag, and we
        // must not strip it.
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let content =
            "Source: foo\nPriority: standard\n\nPackage: foo\nPriority: optional\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            content
        );
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_aggregates_multiple_binaries() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nPriority: optional\nDescription: Foo\n bar\n\nPackage: bar\nPriority: optional\nDescription: Bar\n baz\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Remove redundant Priority: optional from binary stanzas bar, foo."
        );
        assert_eq!(result.fixed_lintian_issues.len(), 2);
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDescription: Foo\n bar\n\nPackage: bar\nDescription: Bar\n baz\n",
        );
    }

    #[test]
    fn test_no_change_when_priority_unset() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let content = "Source: foo\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_no_change_when_priority_non_optional() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let content = "Source: foo\nPriority: standard\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(detect_in(base).unwrap().is_empty());
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
    fn test_diagnostic_carries_correct_info() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nPriority: optional\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let diags = detect_in(base).unwrap();
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert_eq!(
            issue.tag.as_deref(),
            Some("redundant-priority-optional-field")
        );
        assert_eq!(issue.visibility, Some(Visibility::Pedantic));
    }
}
