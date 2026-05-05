use crate::diagnostic::{Action, Diagnostic, FilesystemAction, MakefileAction};
use crate::{FixerError, LintianIssue};
use debian_control::lossless::Control;
use makefile_lossless::Makefile;
use std::path::{Path, PathBuf};
use std::str::FromStr;

fn read_compat(path: &Path) -> Result<Option<u32>, std::io::Error> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(content.trim().parse::<u32>().ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
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

fn rules_dh_compat(rules_abs: &Path) -> Result<Option<u32>, FixerError> {
    if !rules_abs.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(rules_abs)?;
    let makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;
    let result = makefile
        .find_variable("DH_COMPAT")
        .next()
        .and_then(|def| def.raw_value())
        .and_then(|v| v.trim().parse::<u32>().ok());
    Ok(result)
}

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let compat_rel = PathBuf::from("debian/compat");
    let rules_rel = PathBuf::from("debian/rules");

    let control_abs = base_path.join(&control_rel);
    if !control_abs.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&control_abs)?;
    let Ok(control) = Control::from_str(&content) else {
        return Ok(Vec::new());
    };

    let control_compat = control_compat_level(&control);
    let file_compat = read_compat(&base_path.join(&compat_rel))?;

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

    let rules_abs = base_path.join(&rules_rel);
    let rules_compat = rules_dh_compat(&rules_abs)?;
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

declare_fixer! {
    name: "declares-possibly-conflicting-debhelper-compat-versions",
    tags: ["declares-possibly-conflicting-debhelper-compat-versions"],
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
        let v: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &v, &FixerPreferences::default())
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
