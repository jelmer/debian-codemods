use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use std::path::PathBuf;

/// Binary dependency fields lintian's check considers; matches its
/// `is_dep_field` for the per-binary loop.
const BINARY_DEP_FIELDS: &[&str] = &["Depends", "Pre-Depends", "Recommends", "Suggests"];

/// True when a relations field contains a standalone, unversioned `awk`
/// entry (no alternatives). `awk | mawk` is not flagged: removing only the
/// `awk` alternative needs more judgement than this fixer wants to apply,
/// and dropping the whole entry would lose the alternative.
fn has_bare_unversioned_awk(value: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    for entry in relations.entries() {
        let rels: Vec<_> = entry.relations().collect();
        if rels.len() != 1 {
            continue;
        }
        let r = &rels[0];
        if r.try_name().as_deref() == Some("awk") && r.version().is_none() {
            return true;
        }
    }
    false
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

    // The lintian check exempts the `base-files` source package itself.
    if control.source().and_then(|s| s.name()).as_deref() == Some("base-files") {
        return Ok(Vec::new());
    }

    let control_rel = PathBuf::from("debian/control");
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for binary in control.binaries() {
        let Some(pkg_name) = binary.name() else {
            continue;
        };
        for field in BINARY_DEP_FIELDS {
            let Some(value) = binary.as_deb822().get(field) else {
                continue;
            };
            if !has_bare_unversioned_awk(&value) {
                continue;
            }
            let issue = LintianIssue {
                package: Some(pkg_name.clone()),
                package_type: Some(PackageType::Binary),
                visibility: Some(Visibility::Error),
                tag: Some("needlessly-depends-on-awk".to_string()),
                info: Some((*field).to_string()),
            };
            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!("Binary package {} needlessly depends on awk.", pkg_name),
                format!("Drop awk dependency from {}.", pkg_name),
                vec![Action::Deb822(Deb822Action::DropRelation {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: pkg_name.clone(),
                    },
                    field: (*field).to_string(),
                    package: "awk".into(),
                })],
            ));
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut packages: Vec<&str> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Deb822(Deb822Action::DropRelation {
                paragraph: ParagraphSelector::Binary { package },
                ..
            }) => Some(package.as_str()),
            _ => None,
        })
        .collect();
    packages.sort();
    packages.dedup();
    match packages.as_slice() {
        [pkg] => format!("Drop awk dependency from {}.", pkg),
        pkgs => format!("Drop awk dependency from {}.", pkgs.join(", ")),
    }
}

declare_detector! {
    name: "needlessly-depends-on-awk",
    tags: ["needlessly-depends-on-awk"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Pre-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Recommends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Suggests",
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
        let ws = FsWorkspace::new(base, Some("test".into()), Some("1.0".parse().unwrap()));
        detect(&ws, &FixerPreferences::default())
    }

    fn write_control(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), content).unwrap();
    }

    #[test]
    fn test_has_bare_unversioned_awk() {
        assert!(has_bare_unversioned_awk("awk"));
        assert!(has_bare_unversioned_awk("foo, awk, bar"));
    }

    #[test]
    fn test_versioned_awk_not_flagged() {
        // The lintian condition requires an unversioned relation; lintian
        // notes that versioned awk wouldn't work anyway, but we follow the
        // check's letter and skip versioned cases.
        assert!(!has_bare_unversioned_awk("awk (>= 1.0)"));
    }

    #[test]
    fn test_awk_alternative_not_flagged() {
        // `awk | mawk` shouldn't be touched: the right edit is ambiguous
        // (drop awk and keep mawk, or vice versa) and dropping the entry
        // wholesale would lose the alternative.
        assert!(!has_bare_unversioned_awk("awk | mawk"));
        assert!(!has_bare_unversioned_awk("mawk | awk"));
    }

    #[test]
    fn test_no_awk_at_all() {
        assert!(!has_bare_unversioned_awk("foo, bar"));
        assert!(!has_bare_unversioned_awk(""));
    }

    #[test]
    fn test_drops_awk_from_depends() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDepends: awk, libfoo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(result.description, "Drop awk dependency from foo.");
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDepends: libfoo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_drops_awk_from_recommends() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nRecommends: awk\nDescription: Foo\n bar\n",
        );

        run_apply(base).unwrap();
        // When the only relation in the field was awk, the field is removed.
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_base_files_exempted() {
        // lintian's own check exempts the `base-files` source package so
        // base-files can keep its declared awk dependency.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content =
            "Source: base-files\n\nPackage: base-files\nDepends: awk\nDescription: Base\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            content
        );
    }

    #[test]
    fn test_no_change_for_awk_alternative() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content = "Source: foo\n\nPackage: foo\nDepends: awk | mawk\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_no_change_when_no_awk() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content = "Source: foo\n\nPackage: foo\nDepends: libfoo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_handles_multiple_binaries() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDepends: awk\nDescription: Foo\n bar\n\nPackage: bar\nDepends: awk, libbar\nDescription: Bar\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(result.description, "Drop awk dependency from bar, foo.");
        assert_eq!(result.fixed_lintian_issues.len(), 2);
    }
}
