use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_control::lossless::relations::Relations;
use std::path::PathBuf;

const MISC_BU: &str = "${misc:Built-Using}";
const MISC_SBU: &str = "${misc:Static-Built-Using}";

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    // Only act on Go packages.
    let is_go_package = source.build_depends().is_some_and(|bd| {
        bd.entries().any(|or_deps| {
            or_deps.relations().any(|dep| {
                matches!(
                    dep.try_name().as_deref(),
                    Some("golang-go") | Some("golang-any")
                )
            })
        })
    });
    if !is_go_package {
        return Ok(Vec::new());
    }

    let default_architecture = source.architecture();

    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for binary in control.binaries() {
        let Some(binary_name) = binary.name() else {
            continue;
        };
        let p = binary.as_deb822();
        let architecture = p
            .get("Architecture")
            .or_else(|| default_architecture.clone())
            .unwrap_or_else(|| "any".into());

        if architecture == "all" {
            // Drop ${misc:Built-Using} from arch:all packages (if present).
            let Some(built_using_str) = p.get("Built-Using") else {
                continue;
            };
            let (relations, _) = Relations::parse_relaxed(&built_using_str, true);
            let has_misc = relations.substvars().any(|s| s == MISC_BU);
            if !has_misc {
                continue;
            }

            let line_no = p
                .entries()
                .find(|e| e.key().as_deref() == Some("Built-Using"))
                .map(|e| e.line() + 1)
                .unwrap_or_else(|| p.line() + 1);

            let issue = LintianIssue::binary_with_info(
                &binary_name,
                "built-using-field-on-arch-all-package",
                vec![format!(
                    "(in section for {}) [debian/control:{}]",
                    binary_name, line_no
                )],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!(
                    "Remove unnecessary {} from Built-Using for {}.",
                    MISC_BU, binary_name
                ),
                vec![Action::Deb822(Deb822Action::DropSubstvar {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: binary_name.clone(),
                    },
                    field: "Built-Using".into(),
                    substvar: MISC_BU.into(),
                })],
            ));
        } else {
            // Add ${misc:Built-Using} if missing.
            let built_using_str = p.get("Built-Using").unwrap_or_default();
            let (relations, _) = Relations::parse_relaxed(&built_using_str, true);
            let has_misc = relations.substvars().any(|s| s == MISC_BU);
            if !has_misc {
                let line_no = p.line() + 1;
                let issue = LintianIssue::binary_with_info(
                    &binary_name,
                    "missing-built-using-field-for-golang-package",
                    vec![format!(
                        "(in section for {}) [debian/control:{}]",
                        binary_name, line_no
                    )],
                );
                diagnostics.push(Diagnostic::with_actions(
                    issue,
                    format!("Add missing {} to Built-Using on {}.", MISC_BU, binary_name),
                    vec![Action::Deb822(Deb822Action::EnsureSubstvar {
                        file: control_rel.clone(),
                        paragraph: ParagraphSelector::Binary {
                            package: binary_name.clone(),
                        },
                        field: "Built-Using".into(),
                        substvar: MISC_BU.into(),
                    })],
                ));
            }

            // Add ${misc:Static-Built-Using} if missing.
            let static_built_using_str = p.get("Static-Built-Using").unwrap_or_default();
            let (sbu_relations, _) = Relations::parse_relaxed(&static_built_using_str, true);
            let has_misc_static = sbu_relations.substvars().any(|s| s == MISC_SBU);
            if !has_misc_static {
                let line_no = p.line() + 1;
                let issue = LintianIssue::binary_with_info(
                    &binary_name,
                    "missing-static-built-using-field-for-golang-package",
                    vec![format!(
                        "(in section for {}) [debian/control:{}]",
                        binary_name, line_no
                    )],
                );
                diagnostics.push(Diagnostic::with_actions(
                    issue,
                    format!(
                        "Add missing {} to Static-Built-Using on {}.",
                        MISC_SBU, binary_name
                    ),
                    vec![Action::Deb822(Deb822Action::EnsureSubstvar {
                        file: control_rel.clone(),
                        paragraph: ParagraphSelector::Binary {
                            package: binary_name.clone(),
                        },
                        field: "Static-Built-Using".into(),
                        substvar: MISC_SBU.into(),
                    })],
                ));
            }
        }
    }

    Ok(diagnostics)
}

/// Categorise each Built-Using-related action by its (substvar, field,
/// is_drop) shape and collect the binary package name from the
/// paragraph selector.
fn describe_aggregate(_fixed: &[Diagnostic], actions: &[Action]) -> String {
    let mut added: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut added_static: Vec<String> = Vec::new();

    for action in actions {
        let Action::Deb822(deb) = action else {
            continue;
        };
        let (paragraph, field, substvar, is_drop) = match deb {
            Deb822Action::EnsureSubstvar {
                paragraph,
                field,
                substvar,
                ..
            } => (paragraph, field, substvar, false),
            Deb822Action::DropSubstvar {
                paragraph,
                field,
                substvar,
                ..
            } => (paragraph, field, substvar, true),
            _ => continue,
        };
        let ParagraphSelector::Binary { package } = paragraph else {
            continue;
        };
        match (field.as_str(), substvar.as_str(), is_drop) {
            ("Built-Using", MISC_BU, false) => added.push(package.clone()),
            ("Built-Using", MISC_BU, true) => removed.push(package.clone()),
            ("Static-Built-Using", MISC_SBU, false) => added_static.push(package.clone()),
            _ => {}
        }
    }
    added.sort();
    added.dedup();
    removed.sort();
    removed.dedup();
    added_static.sort();
    added_static.dedup();

    let mut parts: Vec<String> = Vec::new();
    if !added.is_empty() && !removed.is_empty() {
        parts.push(format!(
            "Added {} to {} and removed it from {}",
            MISC_BU,
            added.join(", "),
            removed.join(", ")
        ));
    } else if !added.is_empty() {
        parts.push(format!(
            "Add missing {} to Built-Using on {}",
            MISC_BU,
            added.join(", ")
        ));
    } else if !removed.is_empty() {
        parts.push(format!(
            "Remove unnecessary {} for {}",
            MISC_BU,
            removed.join(", ")
        ));
    }
    if !added_static.is_empty() {
        parts.push(format!(
            "Add missing {} to Static-Built-Using on {}",
            MISC_SBU,
            added_static.join(", ")
        ));
    }
    if parts.is_empty() {
        format!("Adjust {} substvars for Go packages.", MISC_BU)
    } else {
        parts.join(". ") + "."
    }
}

declare_detector! {
    name: "built-using-for-golang",
    tags: ["built-using-for-golang", "missing-static-built-using-field-for-golang-package"],
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
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "blah", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_add_built_using_for_golang_package() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control_path = debian.join("control");
        fs::write(
            &control_path,
            "Source: blah\nArchitecture: any\nBuild-Depends: golang-go\n\nPackage: blah\nDescription: blah\n blah\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: blah\nArchitecture: any\nBuild-Depends: golang-go\n\nPackage: blah\nBuilt-Using: ${misc:Built-Using}\nStatic-Built-Using: ${misc:Static-Built-Using}\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_remove_built_using_for_arch_all() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control_path = debian.join("control");
        fs::write(
            &control_path,
            "Source: blah\nArchitecture: any\nBuild-Depends: golang-go\n\nPackage: blah\nArchitecture: all\nBuilt-Using: ${misc:Built-Using}\nDescription: blah\n blah\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: blah\nArchitecture: any\nBuild-Depends: golang-go\n\nPackage: blah\nArchitecture: all\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_no_changes_for_non_go_package() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nArchitecture: any\n\nPackage: blah\nArchitecture: all\nBuilt-Using: ${misc:Built-Using}\nDescription: blah\n blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_unrelated_built_using() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control_path = debian.join("control");
        let original = "Source: blah\nArchitecture: any\nBuild-Depends: golang-go\n\nPackage: blah\nArchitecture: all\nBuilt-Using: ${w32:Built-Using}\nDescription: blah\n blah\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_detects_golang_any() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control_path = debian.join("control");
        fs::write(
            &control_path,
            "Source: blah\nArchitecture: any\nBuild-Depends: golang-any\n\nPackage: blah\nDescription: blah\n blah\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: blah\nArchitecture: any\nBuild-Depends: golang-any\n\nPackage: blah\nBuilt-Using: ${misc:Built-Using}\nStatic-Built-Using: ${misc:Static-Built-Using}\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_add_static_built_using_for_golang_package() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control_path = debian.join("control");
        fs::write(
            &control_path,
            "Source: blah\nArchitecture: any\nBuild-Depends: golang-go\n\nPackage: blah\nBuilt-Using: ${misc:Built-Using}\nDescription: blah\n blah\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: blah\nArchitecture: any\nBuild-Depends: golang-go\n\nPackage: blah\nBuilt-Using: ${misc:Built-Using}\nStatic-Built-Using: ${misc:Static-Built-Using}\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_no_changes_when_static_built_using_exists() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: golang-go\n\nPackage: blah\nArchitecture: any\nBuilt-Using: ${misc:Built-Using}\nStatic-Built-Using: ${misc:Static-Built-Using}\nDescription: blah\n blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
