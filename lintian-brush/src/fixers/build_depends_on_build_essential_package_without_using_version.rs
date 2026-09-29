use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use std::path::PathBuf;

const FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

/// Packages that ship in build-essential (see lintian's
/// data/fields/build-essential-packages). Depending on any of them is
/// redundant unless a specific version is required.
const BUILD_ESSENTIAL_PACKAGES: &[&str] =
    &["libc6-dev", "libc-dev", "gcc", "g++", "make", "dpkg-dev"];

fn is_build_essential(pkg: &str) -> bool {
    BUILD_ESSENTIAL_PACKAGES.contains(&pkg)
}

/// Names of unversioned, single-alternative build-essential entries in
/// `value`. An entry like `gcc (>= 4)` or `gcc | clang` is left alone
/// because the tag only applies to bare, dropable entries.
fn droppable_entries(value: &str) -> Vec<String> {
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    let mut out = Vec::new();
    for entry in relations.entries() {
        let rels: Vec<_> = entry.relations().collect();
        if rels.len() != 1 {
            continue;
        }
        let r = &rels[0];
        if r.version().is_some() {
            continue;
        }
        let Some(name) = r.try_name() else {
            continue;
        };
        if is_build_essential(&name) {
            out.push(name);
        }
    }
    out
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
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    let control_rel = PathBuf::from("debian/control");
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for field in FIELDS {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        for package in droppable_entries(&value) {
            let issue = LintianIssue::source_with_info(
                "build-depends-on-build-essential-package-without-using-version",
                Visibility::Error,
                vec![format!("{} [{}: {}]", package, field, package)],
            );
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    format!(
                        "Unversioned build dependency on build-essential member {}.",
                        package
                    ),
                    format!(
                        "Drop unversioned build dependency on build-essential member {}.",
                        package
                    ),
                    vec![Action::Deb822(Deb822Action::DropRelation {
                        file: control_rel.clone(),
                        paragraph: ParagraphSelector::Source,
                        field: (*field).to_string(),
                        package: package.clone(),
                    })],
                )
                .with_certainty(Certainty::Certain),
            );
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut packages: Vec<&str> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Deb822(Deb822Action::DropRelation { package, .. }) => Some(package.as_str()),
            _ => None,
        })
        .collect();
    packages.sort();
    packages.dedup();

    match packages.as_slice() {
        [pkg] => format!(
            "Drop unversioned build dependency on build-essential member {}.",
            pkg
        ),
        pkgs => format!(
            "Drop unversioned build dependencies on build-essential members {}.",
            pkgs.join(", ")
        ),
    }
}

declare_detector! {
    name: "build-depends-on-build-essential-package-without-using-version",
    tags: ["build-depends-on-build-essential-package-without-using-version"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends-Indep",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends-Arch",
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
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = FsWorkspace::new(base, Some("test".into()), Some(version.clone()));
        adapter.apply(&ws, &FixerPreferences::default())
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
    fn test_droppable_entries_bare_names() {
        assert_eq!(
            droppable_entries("gcc, debhelper-compat (= 13), make"),
            vec!["gcc".to_string(), "make".to_string()]
        );
    }

    #[test]
    fn test_droppable_entries_ignores_versioned() {
        assert!(droppable_entries("gcc (>= 4:7)").is_empty());
        assert!(droppable_entries("make (>= 4)").is_empty());
    }

    #[test]
    fn test_droppable_entries_ignores_alternatives() {
        assert!(droppable_entries("gcc | clang").is_empty());
    }

    #[test]
    fn test_droppable_entries_ignores_non_build_essential() {
        assert!(droppable_entries("debhelper-compat (= 13), libfoo-dev").is_empty());
    }

    #[test]
    fn test_droppable_entries_covers_all_known() {
        for pkg in BUILD_ESSENTIAL_PACKAGES {
            assert_eq!(droppable_entries(pkg), vec![pkg.to_string()]);
        }
    }

    #[test]
    fn test_removes_gcc_from_build_depends() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: gcc, debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: any\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Drop unversioned build dependency on build-essential member gcc.",
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: any\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_aggregates_multiple_packages() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: gcc, make, debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: any\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Drop unversioned build dependencies on build-essential members gcc, make.",
        );
        assert_eq!(result.fixed_lintian_issues.len(), 2);
    }

    #[test]
    fn test_handles_build_depends_indep_and_arch() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends-Indep: make\nBuild-Depends-Arch: gcc\n\nPackage: foo\nArchitecture: any\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 2);
        let content = fs::read_to_string(base.join("debian/control")).unwrap();
        assert!(!content.contains("Build-Depends-Indep"));
        assert!(!content.contains("Build-Depends-Arch"));
    }

    #[test]
    fn test_no_change_when_versioned() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: gcc (>= 4:7), debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: any\nDescription: Foo\n bar\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_no_change_without_build_essential() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: any\nDescription: Foo\n bar\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_without_build_depends() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nArchitecture: any\nDescription: Foo\n bar\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_with_alternative() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: gcc | clang, debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: any\nDescription: Foo\n bar\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
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
            "Source: foo\nBuild-Depends: gcc, debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: any\nDescription: Foo\n bar\n",
        );

        let diags = detect_in(base).unwrap();
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert_eq!(
            issue.tag.as_deref(),
            Some("build-depends-on-build-essential-package-without-using-version"),
        );
        assert_eq!(issue.info.as_deref(), Some("gcc [Build-Depends: gcc]"));
    }
}
