use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::{compat_level, FixerWorkspace};
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType, Visibility};
use debian_control::lossless::relations::Relations;
use std::path::PathBuf;

const SEP: char = '\t';

fn uses_debhelper(build_depends: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(build_depends, true);
    for entry in relations.entries() {
        for relation in entry.relations() {
            let name = relation.try_name();
            if name.as_deref() == Some("debhelper") || name.as_deref() == Some("debhelper-compat") {
                return true;
            }
        }
    }
    false
}

fn has_misc_depends(field_value: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(field_value, true);
    let found = relations.substvars().any(|s| s == "${misc:Depends}");
    found
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");

    // The fix is incompatible with compat >= 14: debhelper auto-injects
    // ${misc:Depends} there.
    if let Some(level) = compat_level(ws)? {
        if level >= 14 {
            return Ok(Vec::new());
        }
    }

    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let uses_dh = control
        .source()
        .and_then(|s| s.build_depends())
        .map(|bd| uses_debhelper(&bd.to_string()))
        .unwrap_or(false);
    if !uses_dh {
        return Ok(Vec::new());
    }

    let mut diagnostics = Vec::new();
    for binary in control.binaries() {
        let Some(package_name) = binary.as_deb822().get("Package") else {
            continue;
        };
        let depends = binary
            .as_deb822()
            .get("Depends")
            .map(|d| d.to_string())
            .unwrap_or_default();
        let pre_depends = binary
            .as_deb822()
            .get("Pre-Depends")
            .map(|d| d.to_string())
            .unwrap_or_default();
        if has_misc_depends(&depends) || has_misc_depends(&pre_depends) {
            continue;
        }

        let line_no = binary.as_deb822().line() + 1;
        let issue = LintianIssue {
            package: Some(package_name.clone()),
            package_type: Some(PackageType::Binary),
            visibility: Some(Visibility::Warning),
            tag: Some("debhelper-but-no-misc-depends".to_string()),
            info: Some(format!(
                "(in section for {}) Depends [debian/control:{}]",
                package_name, line_no
            )),
        };

        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("pkg{}{}", SEP, package_name),
            format!(
                "Add missing ${{misc:Depends}} to Depends for {}.",
                package_name
            ),
            vec![Action::Deb822(Deb822Action::EnsureSubstvar {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Binary {
                    package: package_name.clone(),
                },
                field: "Depends".into(),
                substvar: "${misc:Depends}".into(),
            })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let mut packages: Vec<String> = fixed
        .iter()
        .filter_map(|(d, _)| {
            d.message
                .split_once(SEP)
                .filter(|(tag, _)| *tag == "pkg")
                .map(|(_, pkg)| pkg.to_string())
        })
        .collect();
    packages.sort();
    packages.dedup();
    format!(
        "Add missing ${{misc:Depends}} to Depends for {}.",
        packages.join(", ")
    )
}

declare_detector! {
    name: "debhelper-but-no-misc-depends",
    tags: ["debhelper-but-no-misc-depends"],
    triggers: [
        crate::workspace::Trigger::File("debian/compat"),
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "X-DH-Compat",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Depends",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Pre-Depends",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
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
    fn test_add_misc_depends() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test-package\nBuild-Depends: debhelper (>= 9)\n\nPackage: test-package\nArchitecture: any\nDepends: ${shlibs:Depends}\nDescription: Test package\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-package\nBuild-Depends: debhelper (>= 9)\n\nPackage: test-package\nArchitecture: any\nDepends: ${shlibs:Depends}, ${misc:Depends}\nDescription: Test package\n",
        );
    }

    #[test]
    fn test_already_has_misc_depends() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nBuild-Depends: debhelper (>= 9)\n\nPackage: test-package\nArchitecture: any\nDepends: ${shlibs:Depends}, ${misc:Depends}\nDescription: Test package\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_skip_for_compat_14() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        let initial = "Source: test-package\nBuild-Depends: debhelper-compat (= 14)\n\nPackage: test-package\nArchitecture: any\nDepends: ${shlibs:Depends}\nDescription: Test package\n";
        fs::write(&control, initial).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&control).unwrap(), initial);
    }
}
