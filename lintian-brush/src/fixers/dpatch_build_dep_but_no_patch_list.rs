use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const STUB_CONTENT: &[u8] =
    b"# List patches to apply here\n# Empty file cannot be represented in Debian diff\n";

fn has_dpatch(source: &debian_control::lossless::Source) -> bool {
    let fields = ["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];
    for field in fields {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        let (relations, _errors) =
            debian_control::lossless::relations::Relations::parse_relaxed(&value, true);
        let found = relations.entries().any(|e| {
            e.relations()
                .any(|r| r.try_name().as_deref() == Some("dpatch"))
        });
        if found {
            return true;
        }
    }
    false
}

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_abs = base_path.join(&control_rel);
    if !control_abs.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&control_abs)?;
    let Ok(control) = Control::from_str(&content) else {
        return Ok(Vec::new());
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    if !has_dpatch(&source) {
        return Ok(Vec::new());
    }

    // If there's already an existing patch list (any file starting with
    // "00list" in debian/patches) we don't need to write a stub.
    let patches_dir = base_path.join("debian/patches");
    if patches_dir.exists() {
        if let Ok(read_dir) = std::fs::read_dir(&patches_dir) {
            let has_list = read_dir
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with("00list"));
            if has_list {
                return Ok(Vec::new());
            }
        }
    }

    let issue = LintianIssue::source_with_info("dpatch-build-dep-but-no-patch-list", vec![]);

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Add missing debian/patches/00list file for dpatch.",
        vec![Action::Filesystem(FilesystemAction::Write {
            file: PathBuf::from("debian/patches/00list"),
            content: STUB_CONTENT.to_vec(),
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_fixer! {
    name: "dpatch-build-dep-but-no-patch-list",
    tags: ["dpatch-build-dep-but-no-patch-list"],
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
    fn test_creates_00list_when_dpatch_in_build_depends() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nMaintainer: Test User <test@example.com>\nBuild-Depends: debhelper, dpatch\n\nPackage: test-package\nDescription: Test package\n Test description\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        let list = tmp.path().join("debian/patches/00list");
        assert_eq!(fs::read(&list).unwrap(), STUB_CONTENT,);
    }

    #[test]
    fn test_no_changes_when_no_dpatch() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nMaintainer: Test User <test@example.com>\nBuild-Depends: debhelper\n\nPackage: test-package\nDescription: Test package\n Test description\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_00list_exists() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        let patches = debian.join("patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nMaintainer: Test User <test@example.com>\nBuild-Depends: debhelper, dpatch\n\nPackage: test-package\nDescription: Test package\n Test description\n",
        )
        .unwrap();
        fs::write(patches.join("00list"), "# existing\n").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_dpatch_in_build_depends_indep() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nMaintainer: Test User <test@example.com>\nBuild-Depends: debhelper\nBuild-Depends-Indep: dpatch\n\nPackage: test-package\nDescription: Test package\n Test description\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read(tmp.path().join("debian/patches/00list")).unwrap(),
            STUB_CONTENT,
        );
    }
}
