use crate::diagnostic::{Action, Deb822Action, Diagnostic, FilesystemAction, ParagraphSelector};
use crate::{Certainty, FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_abs = base_path.join(&control_rel);
    if !control_abs.exists() {
        return Ok(Vec::new());
    }

    // The fixer is conditional on a debian/patches directory: dpatch
    // implies a patch list, so we don't migrate when the package isn't
    // actually carrying patches.
    let patches_dir = base_path.join("debian/patches");
    if !patches_dir.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&control_abs)?;
    let Ok(control) = Control::from_str(&content) else {
        return Ok(Vec::new());
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

    let issue =
        LintianIssue::source_with_info("package-uses-deprecated-dpatch-patch-system", vec![]);

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
    let list_file = patches_dir.join("00list");
    let series_file = patches_dir.join("series");
    let renamed_list = list_file.exists() && !series_file.exists();
    if renamed_list {
        actions.push(Action::Filesystem(FilesystemAction::Rename {
            file: PathBuf::from("debian/patches/00list"),
            to: PathBuf::from("debian/patches/series"),
        }));
    }

    let description = if renamed_list {
        "Migrate from dpatch to 3.0 (quilt) source format. Remove dpatch from Build-Depends. Set source format to 3.0 (quilt). Rename debian/patches/00list to series"
    } else {
        "Migrate from dpatch to 3.0 (quilt) source format. Remove dpatch from Build-Depends. Set source format to 3.0 (quilt)"
    };

    Ok(vec![
        Diagnostic::with_actions(issue, description, actions).with_certainty(Certainty::Certain)
    ])
}

declare_fixer! {
    name: "package-uses-deprecated-dpatch-patch-system",
    tags: ["package-uses-deprecated-dpatch-patch-system"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test-package", &version, &FixerPreferences::default())
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
