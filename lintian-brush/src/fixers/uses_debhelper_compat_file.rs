use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, FilesystemAction, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_analyzer::debhelper::highest_stable_compat_level;
use debian_analyzer::relations::is_relation_implied;
use debian_control::lossless::Entry;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

fn check_cdbs(ws: &dyn FixerWorkspace) -> bool {
    let Ok(Some(content)) = ws.read_file(Path::new("debian/rules")) else {
        return false;
    };
    content
        .windows(b"/usr/share/cdbs/".len())
        .any(|w| w == b"/usr/share/cdbs/")
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let compat_rel = PathBuf::from("debian/compat");
    let compat_bytes = match ws.read_file(&compat_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(compat_text) = std::str::from_utf8(&compat_bytes) else {
        return Ok(Vec::new());
    };
    let Ok(compat_version) = compat_text.trim().parse::<u8>() else {
        return Ok(Vec::new());
    };

    if compat_version < 11 {
        return Ok(Vec::new());
    }
    if check_cdbs(ws) {
        return Ok(Vec::new());
    }
    if compat_version > highest_stable_compat_level() {
        return Ok(Vec::new());
    }

    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    let target_str = format!("debhelper (>= {})", compat_version);
    let target_entry = Entry::from_str(&target_str)
        .map_err(|e| FixerError::Other(format!("Failed to parse target entry: {:?}", e)))?;

    let issue = LintianIssue::source_with_info(
        "uses-debhelper-compat-file",
        Visibility::Warning,
        vec!["[debian/compat]".to_string()],
    );

    // Per field: drop debhelper if its constraint is implied by
    // `debhelper (>= compat_version)`. Always add debhelper-compat to
    // Build-Depends and remove the debian/compat file — those are the
    // primary fix; the DropRelation actions only fire when a debhelper
    // entry is redundant given the new debhelper-compat dependency.
    let mut actions: Vec<Action> = Vec::new();
    for field in FIELDS {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        let (relations, _errors) =
            debian_control::lossless::relations::Relations::parse_relaxed(&value, true);
        let Ok((_pos, existing)) = relations.get_relation("debhelper") else {
            continue;
        };
        if !is_relation_implied(&existing, &target_entry) {
            continue;
        }
        actions.push(Action::Deb822(Deb822Action::DropRelation {
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Source,
            field: (*field).to_string(),
            package: "debhelper".into(),
        }));
    }

    actions.push(Action::Deb822(Deb822Action::EnsureRelation {
        file: control_rel.clone(),
        paragraph: ParagraphSelector::Source,
        field: "Build-Depends".into(),
        entry: format!("debhelper-compat (= {})", compat_version),
    }));
    actions.push(Action::Filesystem(FilesystemAction::Delete {
        file: compat_rel,
    }));

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Package uses debian/compat instead of debhelper-compat.",
        "Set debhelper-compat version in Build-Depends.",
        actions,
    )])
}

declare_detector! {
    name: "uses-debhelper-compat-file",
    tags: ["uses-debhelper-compat-file"],
    triggers: [
        crate::workspace::Trigger::File("debian/compat"),
        crate::workspace::Trigger::File("debian/rules"),
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends-Indep",
        },
        crate::workspace::Trigger::Deb822Field {
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
    fn test_simple() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("compat"), "11\n").unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: f2fs-tools\nBuild-Depends:\n debhelper (>= 11),\n pkg-config\n\nPackage: blah\nArchitecture: any\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(!debian.join("compat").exists());
        // The deb822 lossless layout collapses two entries onto one line
        // when the result fits; the integration fixtures (3+ entries)
        // exercise the multi-line case. This matches the pre-port
        // behaviour of `set_build_depends`.
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: f2fs-tools\nBuild-Depends:\n debhelper-compat (= 11), pkg-config\n\nPackage: blah\nArchitecture: any\n",
        );
    }

    #[test]
    fn test_no_change_when_compat_too_old() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("compat"), "9\n").unwrap();
        fs::write(
            debian.join("control"),
            "Source: test\nBuild-Depends: debhelper (>= 9)\n\nPackage: test\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert!(debian.join("compat").exists());
    }
}
