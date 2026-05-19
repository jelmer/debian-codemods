use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

fn is_go_package(source: &debian_control::lossless::Source) -> bool {
    let Some(build_depends) = source.build_depends() else {
        return false;
    };
    let found = build_depends.entries().any(|or_deps| {
        or_deps.relations().any(|dep| {
            matches!(
                dep.try_name().as_deref(),
                Some("golang-go") | Some("golang-any") | Some("dh-golang")
            )
        })
    });
    found
}

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
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    if !is_go_package(&source) {
        return Ok(Vec::new());
    }

    let default_architecture = source.architecture();

    let mut diagnostics = Vec::new();
    for binary in control.binaries() {
        let Some(binary_name) = binary.as_deb822().get("Package") else {
            continue;
        };
        let architecture = binary
            .architecture()
            .or_else(|| default_architecture.clone())
            .unwrap_or_else(|| "any".to_string());
        if architecture == "all" {
            continue;
        }

        let static_built_using = binary
            .as_deb822()
            .get("Static-Built-Using")
            .map(|s| s.to_string())
            .unwrap_or_default();
        let (relations, _) = debian_control::lossless::relations::Relations::parse_relaxed(
            &static_built_using,
            true,
        );
        let has_misc = relations.entries().any(|or_deps| {
            or_deps
                .relations()
                .any(|dep| dep.try_name().as_deref() == Some("${misc:Static-Built-Using}"))
        });
        if has_misc {
            continue;
        }

        let line_no = binary.as_deb822().line() + 1;
        let issue = LintianIssue {
            package: Some(binary_name.clone()),
            package_type: Some(PackageType::Binary),
            visibility: Some(Visibility::Info),
            tag: Some("missing-static-built-using-field-for-golang-package".to_string()),
            info: Some(format!(
                "(in section for {}) [debian/control:{}]",
                binary_name, line_no
            )),
        };

        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!(
                "Static-Built-Using field is missing for Go package {}.",
                binary_name
            ),
            format!(
                "Add ${{misc:Static-Built-Using}} to Static-Built-Using on {}.",
                binary_name
            ),
            vec![Action::Deb822(Deb822Action::EnsureSubstvar {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Binary {
                    package: binary_name.clone(),
                },
                field: "Static-Built-Using".into(),
                substvar: "${misc:Static-Built-Using}".into(),
            })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let mut packages: Vec<String> = fixed
        .iter()
        .filter_map(|(d, _)| d.issue.as_ref()?.package.clone())
        .collect();
    packages.sort();
    packages.dedup();
    format!(
        "Add missing ${{misc:Static-Built-Using}} to Static-Built-Using on {}.",
        packages.join(", ")
    )
}

declare_detector! {
    name: "missing-static-built-using-field-for-golang-package",
    tags: ["missing-static-built-using-field-for-golang-package"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Architecture",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Architecture",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Static-Built-Using",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_simple() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: golang-test\nBuild-Depends: dh-golang\n\nPackage: golang-test\nArchitecture: any\nDescription: Test package for Go\n Test description\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: golang-test\nBuild-Depends: dh-golang\n\nPackage: golang-test\nArchitecture: any\nStatic-Built-Using: ${misc:Static-Built-Using}\nDescription: Test package for Go\n Test description\n",
        );
    }

    #[test]
    fn test_no_change_for_non_go() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\n\nPackage: foo\nArchitecture: any\nDescription: x\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_for_arch_all() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\nBuild-Depends: dh-golang\n\nPackage: foo\nArchitecture: all\nDescription: x\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_has_substvar() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\nBuild-Depends: dh-golang\n\nPackage: foo\nArchitecture: any\nStatic-Built-Using: ${misc:Static-Built-Using}\nDescription: x\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
