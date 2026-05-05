use crate::diagnostic::{Action, Deb822Action, Diagnostic, FilesystemAction, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_analyzer::debhelper::{highest_stable_compat_level, read_debhelper_compat_file};
use debian_analyzer::relations::is_relation_implied;
use debian_control::lossless::{Control, Entry};
use std::path::{Path, PathBuf};
use std::str::FromStr;

const FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

fn check_cdbs(base_path: &Path) -> bool {
    let rules_path = base_path.join("debian/rules");
    if let Ok(content) = std::fs::read_to_string(&rules_path) {
        content.contains("/usr/share/cdbs/")
    } else {
        false
    }
}

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let compat_rel = PathBuf::from("debian/compat");
    let compat_abs = base_path.join(&compat_rel);
    let Some(compat_version) = read_debhelper_compat_file(&compat_abs)? else {
        return Ok(Vec::new());
    };

    if compat_version < 11 {
        return Ok(Vec::new());
    }
    if check_cdbs(base_path) {
        return Ok(Vec::new());
    }
    if compat_version > highest_stable_compat_level() {
        return Ok(Vec::new());
    }

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

    let target_str = format!("debhelper (>= {})", compat_version);
    let target_entry = Entry::from_str(&target_str)
        .map_err(|e| FixerError::Other(format!("Failed to parse target entry: {:?}", e)))?;

    let issue = LintianIssue::source_with_info(
        "uses-debhelper-compat-file",
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
        "Set debhelper-compat version in Build-Depends.",
        actions,
    )])
}

declare_fixer! {
    name: "uses-debhelper-compat-file",
    tags: ["uses-debhelper-compat-file"],
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
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
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
