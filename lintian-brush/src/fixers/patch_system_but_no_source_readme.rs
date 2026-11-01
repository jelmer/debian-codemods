use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const BUILD_DEP_FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

/// Patch systems that we know how to write a README.source for.
///
/// Lintian only emits patch-system-but-no-source-readme when it identifies
/// quilt as the patch system, so quilt is the only system we actually fix
/// here. The enum exists so that if the identified system is anything else
/// (or ambiguous), we back off rather than guess.
#[derive(Debug, PartialEq, Eq)]
enum PatchSystem {
    Quilt,
}

impl PatchSystem {
    fn readme_source_contents(&self) -> &'static [u8] {
        match self {
            PatchSystem::Quilt => b"This package uses quilt to manage all modifications to the upstream\nsource.  See /usr/share/doc/quilt/README.source for details on how to\nuse these.\n",
        }
    }

    fn documentation_path(&self) -> &'static str {
        match self {
            PatchSystem::Quilt => "/usr/share/doc/quilt/README.source",
        }
    }
}

fn build_dep_packages(control: &debian_control::lossless::Control) -> Vec<String> {
    let Some(source) = control.source() else {
        return Vec::new();
    };
    let mut packages = Vec::new();
    for field in BUILD_DEP_FIELDS {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        let (relations, _errors) =
            debian_control::lossless::relations::Relations::parse_relaxed(&value, true);
        for entry in relations.entries() {
            for rel in entry.relations() {
                if let Some(name) = rel.try_name() {
                    packages.push(name.to_string());
                }
            }
        }
    }
    packages
}

/// Identify which patch system is in use, or `None` if we can't tell.
///
/// Returns `None` when either no known system is listed, when more than one
/// is listed (e.g. a package in the middle of migrating from dpatch to
/// quilt), or when the identified system isn't one we have a template for.
fn identify_patch_system(build_deps: &[String]) -> Option<PatchSystem> {
    // Known patch-system build-dependency names, including systems we don't
    // have templates for - we need to see them to detect ambiguity.
    const KNOWN_SYSTEMS: &[&str] = &["quilt", "dpatch", "dbs"];
    let mut listed: Vec<&str> = KNOWN_SYSTEMS
        .iter()
        .copied()
        .filter(|name| build_deps.iter().any(|p| p == name))
        .collect();
    if listed.len() != 1 {
        return None;
    }
    match listed.pop()? {
        "quilt" => Some(PatchSystem::Quilt),
        _ => None,
    }
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    // lintian suppresses the tag when the source format is 3.0 (quilt);
    // dpkg-source handles patches implicitly in that case.
    if ws.source_format()?.as_deref() == Some("3.0 (quilt)") {
        return Ok(Vec::new());
    }

    let readme_rel = PathBuf::from("debian/README.source");
    if ws.read_file(&readme_rel)?.is_some() {
        return Ok(Vec::new());
    }

    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let build_deps = build_dep_packages(&control);

    // Match lintian: it only fires the tag when quilt is the identified
    // patch system (either 3.0 (quilt) format - already excluded - or a
    // quilt build-dep).
    if !build_deps.iter().any(|p| p == "quilt") {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "patch-system-but-no-source-readme",
        Visibility::Warning,
        vec![],
    );
    let description = "Package uses a patch system but has no debian/README.source.";

    // If more than one patch system is listed in the build-deps we can't
    // tell which one is actually driving the build; still report the
    // issue, but emit no action plan so the user can decide what to write.
    let Some(system) = identify_patch_system(&build_deps) else {
        return Ok(vec![Diagnostic::with_plans(issue, description, vec![])]);
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        description,
        format!(
            "Add debian/README.source referring to {}.",
            system.documentation_path()
        ),
        vec![Action::Filesystem(FilesystemAction::Write {
            file: readme_rel,
            content: system.readme_source_contents().to_vec(),
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "patch-system-but-no-source-readme",
    tags: ["patch-system-but-no-source-readme"],
    triggers: [
        debian_workspace::Trigger::File("debian/README.source"),
        debian_workspace::Trigger::File("debian/source/format"),
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
        let version: Version = "1.0-1".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test-package".into()),
            Some(version.clone()),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_control(debian: &Path, build_depends: &str) {
        fs::write(
            debian.join("control"),
            format!(
                "Source: blah\nMaintainer: Joe <joe@example.com>\nBuild-Depends: {}\n\nPackage: blah\nDescription: blah\n blah\n",
                build_depends
            ),
        )
        .unwrap();
    }

    #[test]
    fn test_identify_patch_system_quilt() {
        assert_eq!(
            identify_patch_system(&["quilt".to_string()]),
            Some(PatchSystem::Quilt)
        );
    }

    #[test]
    fn test_identify_patch_system_dpatch_only_unsupported() {
        assert_eq!(identify_patch_system(&["dpatch".to_string()]), None);
    }

    #[test]
    fn test_identify_patch_system_ambiguous_quilt_and_dpatch() {
        assert_eq!(
            identify_patch_system(&["quilt".to_string(), "dpatch".to_string()]),
            None
        );
    }

    #[test]
    fn test_identify_patch_system_no_patch_systems_listed() {
        assert_eq!(
            identify_patch_system(&["debhelper".to_string(), "cdbs".to_string()]),
            None
        );
    }

    #[test]
    fn test_creates_readme_source_when_quilt_build_dep() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        write_control(&debian, "debhelper, quilt");

        run_apply(tmp.path()).unwrap();
        let contents = fs::read_to_string(debian.join("README.source")).unwrap();
        assert_eq!(
            contents,
            "This package uses quilt to manage all modifications to the upstream\nsource.  See /usr/share/doc/quilt/README.source for details on how to\nuse these.\n",
        );
    }

    #[test]
    fn test_detects_quilt_in_build_depends_indep() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper\nBuild-Depends-Indep: quilt\n\nPackage: blah\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(debian.join("README.source").exists());
    }

    #[test]
    fn test_no_change_when_3_0_quilt_format() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        let source = debian.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("format"), "3.0 (quilt)\n").unwrap();
        write_control(&debian, "debhelper, quilt");

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_readme_source_exists() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("README.source"), "pre-existing\n").unwrap();
        write_control(&debian, "debhelper, quilt");

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_without_quilt_build_dep() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        write_control(&debian, "debhelper");

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_with_ambiguous_patch_systems() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        write_control(&debian, "debhelper, quilt, dpatch");

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_detects_but_does_not_fix_ambiguous_patch_systems() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        write_control(&debian, "debhelper, quilt, dpatch");

        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("test-package".into()),
            Some("1.0-1".parse().unwrap()),
        );
        let diagnostics = detect(&ws, &FixerPreferences::default()).unwrap();
        assert_eq!(diagnostics.len(), 1);
        let diag = &diagnostics[0];
        assert_eq!(
            diag.issue.as_ref().and_then(|i| i.tag.as_deref()),
            Some("patch-system-but-no-source-readme")
        );
        assert_eq!(diag.plans, vec![]);
    }

    #[test]
    fn test_fires_for_3_0_native_with_quilt_build_dep() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        let source = debian.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("format"), "3.0 (native)\n").unwrap();
        write_control(&debian, "debhelper, quilt");

        run_apply(tmp.path()).unwrap();
        assert!(debian.join("README.source").exists());
    }
}
