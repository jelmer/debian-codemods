use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction, MakefileAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_control::lossless::Control;
use makefile_lossless::Makefile;
use std::path::{Path, PathBuf};

fn read_compat(ws: &dyn FixerWorkspace) -> Result<Option<u32>, FixerError> {
    let Some(bytes) = ws.read_file(Path::new("debian/compat"))? else {
        return Ok(None);
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(None);
    };
    Ok(content.trim().parse::<u32>().ok())
}

fn control_compat_level(control: &Control) -> Option<u32> {
    let source = control.source()?;
    let build_depends = source.build_depends()?;
    for entry in build_depends.entries() {
        for relation in entry.relations() {
            if relation.try_name().as_deref() != Some("debhelper-compat") {
                continue;
            }
            if let Some((constraint, version)) = relation.version() {
                if constraint.to_string() == "=" {
                    return version.to_string().parse::<u32>().ok();
                }
            }
        }
    }
    None
}

fn rules_dh_compat(ws: &dyn FixerWorkspace) -> Result<Option<u32>, FixerError> {
    let Some(bytes) = ws.read_file(Path::new("debian/rules"))? else {
        return Ok(None);
    };
    let makefile = Makefile::read_relaxed(bytes.as_slice())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;
    let result = makefile
        .find_variable("DH_COMPAT")
        .next()
        .and_then(|def| def.raw_value())
        .and_then(|v| v.trim().parse::<u32>().ok());
    Ok(result)
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let _control_rel = PathBuf::from("debian/control");
    let compat_rel = PathBuf::from("debian/compat");
    let rules_rel = PathBuf::from("debian/rules");

    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let control_compat = control_compat_level(&control);
    let file_compat = read_compat(ws)?;

    let mut actions: Vec<Action> = Vec::new();
    let (compat_version, compat_source) = match (control_compat, file_compat) {
        (Some(cv), Some(_)) => {
            actions.push(Action::Filesystem(FilesystemAction::Delete {
                file: compat_rel,
            }));
            (Some(cv), "debian/control")
        }
        (Some(cv), None) => (Some(cv), "debian/control"),
        (None, Some(fv)) => (Some(fv), "debian/compat"),
        (None, None) => return Ok(Vec::new()),
    };

    let rules_compat = rules_dh_compat(ws)?;
    let mut issue: Option<LintianIssue> = None;
    if let (Some(rules_v), Some(target)) = (rules_compat, compat_version) {
        if rules_v != target {
            issue = Some(LintianIssue::source_with_info(
                "declares-possibly-conflicting-debhelper-compat-versions",
                vec![format!(
                    "{} vs elsewhere {} [{}]",
                    rules_v, target, compat_source
                )],
            ));
        }
        actions.push(Action::Makefile(MakefileAction::RemoveVariable {
            file: rules_rel,
            name: "DH_COMPAT".into(),
        }));
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let message = "Avoid setting debhelper compat version in debian/rules and debian/compat.";
    let diagnostic = if let Some(issue) = issue {
        Diagnostic::with_actions(issue, message, actions)
    } else {
        Diagnostic::untagged(message, actions)
    };
    Ok(vec![diagnostic])
}

declare_detector! {
    name: "declares-possibly-conflicting-debhelper-compat-versions",
    tags: ["declares-possibly-conflicting-debhelper-compat-versions"],
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
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_both_compat_sources_exist() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper-compat (= 10)\n\nPackage: blah\nDescription: blah\n blah\n",
        )
        .unwrap();
        fs::write(debian.join("compat"), "11\n").unwrap();
        fs::write(debian.join("rules"), "#!/usr/bin/make -f\n%:\n\tdh $@\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Avoid setting debhelper compat version in debian/rules and debian/compat."
        );
        assert!(!debian.join("compat").exists());
    }

    #[test]
    fn test_conflicting_dh_compat_in_rules() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper (>= 10.1)\n\nPackage: blah\nDescription: blah\n blah\n",
        )
        .unwrap();
        fs::write(debian.join("compat"), "11\n").unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\nexport DH_COMPAT = 10\n\n%:\n\tdh $@\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n\n%:\n\tdh $@\n",
        );
    }
}
