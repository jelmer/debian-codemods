use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let compat_bytes = match ws.read_file(Path::new("debian/compat"))? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(compat_text) = std::str::from_utf8(&compat_bytes) else {
        return Ok(Vec::new());
    };
    let Ok(minimum_version) = compat_text.trim().parse::<u8>() else {
        return Ok(Vec::new());
    };

    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    // Already at or above the minimum?
    let mut build_depends = source.build_depends().unwrap_or_default();
    let original = build_depends.to_string();
    let version = debversion::Version::from_str(&format!("{}~", minimum_version))
        .map_err(|e| FixerError::Other(format!("Failed to parse version: {:?}", e)))?;
    build_depends.ensure_minimum_version("debhelper", &version);
    if build_depends.to_string() == original {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "no-versioned-debhelper-prerequisite",
        vec![minimum_version.to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Versioned debhelper build-dependency is missing.",
        format!(
            "Bump debhelper dependency to >= {}, since that's what is used in debian/compat.",
            minimum_version
        ),
        vec![Action::Deb822(Deb822Action::EnsureRelation {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            entry: format!("debhelper (>= {}~)", minimum_version),
        })],
    )])
}

declare_detector! {
    name: "package-needs-versioned-debhelper-build-depends",
    tags: ["no-versioned-debhelper-prerequisite"],
    triggers: [
        crate::workspace::Trigger::File("debian/compat"),
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
    ],
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
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_bump_debhelper() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("compat"), "12\n").unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: blah\nMaintainer: Joe <joe@example.com>\nBuild-Depends: debhelper (>= 9), pkg-config\n\nPackage: blah\nDescription: blah\n blah\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\nMaintainer: Joe <joe@example.com>\nBuild-Depends: debhelper (>= 12~), pkg-config\n\nPackage: blah\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_no_change_when_already_correct() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("compat"), "12\n").unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nMaintainer: Joe <joe@example.com>\nBuild-Depends: debhelper (>= 12~)\n\nPackage: blah\nDescription: blah\n blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_compat() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper\n\nPackage: blah\nDescription: blah\n blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
