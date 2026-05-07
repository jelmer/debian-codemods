use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use std::path::{Path, PathBuf};

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

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    if !has_dpatch(&source) {
        return Ok(Vec::new());
    }

    // If there's already an existing patch list (any file starting with
    // "00list" in debian/patches) we don't need to write a stub.
    if let Some(entries) = ws.list_dir(Path::new("debian/patches"))? {
        if entries.iter().any(|n| n.starts_with("00list")) {
            return Ok(Vec::new());
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

declare_detector! {
    name: "dpatch-build-dep-but-no-patch-list",
    tags: ["dpatch-build-dep-but-no-patch-list"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test-package", &version, &FixerPreferences::default())
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
