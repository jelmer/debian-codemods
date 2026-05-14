use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, FilesystemAction, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

const FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");

    // The fixer is conditional on a debian/patches directory: dpatch
    // implies a patch list, so we don't migrate when the package isn't
    // actually carrying patches.
    let patches_entries = match ws.list_dir(Path::new("debian/patches"))? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };

    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    // Find which fields actually contain dpatch.
    let mut drop_fields = Vec::new();
    for field in FIELDS {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        let (relations, _errors) =
            debian_control::lossless::relations::Relations::parse_relaxed(&value, true);
        let has_dpatch = relations.entries().any(|e| {
            e.relations()
                .any(|r| r.try_name().as_deref() == Some("dpatch"))
        });
        if has_dpatch {
            drop_fields.push((*field).to_string());
        }
    }
    if drop_fields.is_empty() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "package-uses-deprecated-dpatch-patch-system",
        Visibility::Warning,
        vec![],
    );

    let mut actions: Vec<Action> = drop_fields
        .iter()
        .map(|field| {
            Action::Deb822(Deb822Action::DropRelation {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: field.clone(),
                package: "dpatch".into(),
            })
        })
        .collect();

    // Set source format to 3.0 (quilt). The applier's Write creates the
    // parent directory if missing (debian/source/).
    actions.push(Action::Filesystem(FilesystemAction::Write {
        file: PathBuf::from("debian/source/format"),
        content: b"3.0 (quilt)\n".to_vec(),
    }));

    // If 00list exists and series doesn't, rename. Otherwise leave the
    // existing series alone.
    let has_list = patches_entries.iter().any(|n| n == "00list");
    let has_series = patches_entries.iter().any(|n| n == "series");
    let renamed_list = has_list && !has_series;
    if renamed_list {
        actions.push(Action::Filesystem(FilesystemAction::Rename {
            file: PathBuf::from("debian/patches/00list"),
            to: PathBuf::from("debian/patches/series"),
        }));
    }

    let label = if renamed_list {
        "Migrate from dpatch to 3.0 (quilt) source format. Remove dpatch from Build-Depends. Set source format to 3.0 (quilt). Rename debian/patches/00list to series"
    } else {
        "Migrate from dpatch to 3.0 (quilt) source format. Remove dpatch from Build-Depends. Set source format to 3.0 (quilt)"
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Package uses deprecated dpatch patch system.",
        label,
        actions,
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "package-uses-deprecated-dpatch-patch-system",
    tags: ["package-uses-deprecated-dpatch-patch-system"],
    triggers: [
        debian_workspace::Trigger::Glob("debian/patches/*"),
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
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test-package".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_migrates_from_dpatch() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        let patches = debian.join("patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nMaintainer: Test User <test@example.com>\nBuild-Depends: debhelper (>= 11), dpatch\n\nPackage: test-package\nArchitecture: any\nDescription: Test package\n A test package.\n",
        )
        .unwrap();
        fs::write(patches.join("00list"), "01-test.patch\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            "Source: test-package\nMaintainer: Test User <test@example.com>\nBuild-Depends: debhelper (>= 11)\n\nPackage: test-package\nArchitecture: any\nDescription: Test package\n A test package.\n",
        );
        assert_eq!(
            fs::read_to_string(debian.join("source/format")).unwrap(),
            "3.0 (quilt)\n"
        );
        assert!(!patches.join("00list").exists());
        assert_eq!(
            fs::read_to_string(patches.join("series")).unwrap(),
            "01-test.patch\n"
        );
    }

    #[test]
    fn test_no_changes_without_patches_dir() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test\nBuild-Depends: dpatch\n\nPackage: test\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_without_dpatch_dependency() {
        let tmp = TempDir::new().unwrap();
        let patches = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(
            tmp.path().join("debian/control"),
            "Source: test\nBuild-Depends: debhelper\n\nPackage: test\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
