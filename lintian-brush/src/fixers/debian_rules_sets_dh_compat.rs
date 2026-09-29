use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, MakefileAction, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

const VAR_NAME: &str = "DH_COMPAT";
const DESCRIPTION: &str = "debian/rules sets DH_COMPAT globally.";
const LABEL: &str = "Remove global DH_COMPAT assignment from debian/rules.";

/// The policy-mandated targets lintian tracks. lintian only reports
/// DH_COMPAT assignments that appear before any of these targets is
/// defined (its `unless (%seen)` guard).
fn is_policy_target(target: &str) -> bool {
    matches!(
        target,
        "build"
            | "binary"
            | "binary-arch"
            | "binary-indep"
            | "clean"
            | "build-arch"
            | "build-indep"
    )
}

fn read_compat_file(ws: &dyn Workspace) -> Result<Option<u32>, FixerError> {
    let Some(bytes) = ws.read_file(Path::new("debian/compat"))? else {
        return Ok(None);
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Ok(None);
    };
    Ok(text.trim().parse::<u32>().ok())
}

/// Read the compat level declared via `debhelper-compat (= N)` in
/// `Build-Depends` or `Build-Depends-Indep` / `Build-Depends-Arch`.
fn control_compat_level(ws: &dyn Workspace) -> Result<Option<u32>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let Some(source) = control.source() else {
        return Ok(None);
    };
    for field in ["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"] {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        let (relations, _errors) =
            debian_control::lossless::relations::Relations::parse_relaxed(&value, true);
        for entry in relations.entries() {
            for rel in entry.relations() {
                if rel.try_name().as_deref() != Some("debhelper-compat") {
                    continue;
                }
                if let Some((op, version)) = rel.version() {
                    if op.to_string() != "=" {
                        continue;
                    }
                    if let Ok(v) = version.to_string().parse::<u32>() {
                        return Ok(Some(v));
                    }
                }
            }
        }
    }
    Ok(None)
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    // Match lintian's `unless (%seen)` guard: only flag assignments that
    // appear before any policy target is introduced.
    let mut boundary: Option<usize> = None;
    for rule in makefile.rules() {
        let line = rule.line();
        let is_phony = rule.targets().any(|t| t.trim() == ".PHONY");
        let introduces_target = if is_phony {
            rule.prerequisites().any(|t| is_policy_target(t.trim()))
        } else {
            rule.targets().any(|t| is_policy_target(t.trim()))
        };
        if introduces_target {
            boundary = Some(boundary.map_or(line, |b| b.min(line)));
        }
    }

    let mut issues: Vec<(LintianIssue, Option<u32>)> = Vec::new();
    for var in makefile.find_variable(VAR_NAME) {
        // lintian's pattern is `\s*(?:export\s+)?DH_COMPAT\s*:?=`, so it
        // matches `=` and `:=` but not `?=` or `+=`.
        if !matches!(var.assignment_operator().as_deref(), Some("=") | Some(":=")) {
            continue;
        }
        if boundary.is_some_and(|b| var.line() >= b) {
            continue;
        }
        let line_no = var.line() + 1;
        let value = var.raw_value().and_then(|v| v.trim().parse::<u32>().ok());
        issues.push((
            LintianIssue::source_with_info(
                "debian-rules-sets-DH_COMPAT",
                Visibility::Warning,
                vec![format!("[debian/rules:{}]", line_no)],
            ),
            value,
        ));
    }

    if issues.is_empty() {
        return Ok(Vec::new());
    }

    // Decide how to migrate the compat level away from debian/rules.
    // Never override an existing compat source; if the versions differ,
    // leave the conflict to the dedicated fixer.
    let file_compat = read_compat_file(ws)?;
    let control_compat = control_compat_level(ws)?;
    let existing_compat = file_compat.or(control_compat);
    let rules_value = issues[0].1;

    let can_remove = !matches!(
        (existing_compat, rules_value),
        (Some(existing), Some(rules_v)) if existing != rules_v
    );

    let migration_value = if existing_compat.is_none() {
        rules_value
    } else {
        None
    };

    let mut diagnostics = Vec::new();
    for (i, (issue, _)) in issues.into_iter().enumerate() {
        let actions = if i == 0 && can_remove {
            let mut acts: Vec<Action> = Vec::new();
            if let Some(level) = migration_value {
                acts.push(Action::Deb822(Deb822Action::EnsureRelation {
                    file: PathBuf::from("debian/control"),
                    paragraph: ParagraphSelector::Source,
                    field: "Build-Depends".into(),
                    entry: format!("debhelper-compat (= {})", level),
                }));
            }
            acts.push(Action::Makefile(MakefileAction::RemoveVariable {
                file: PathBuf::from("debian/rules"),
                name: VAR_NAME.to_string(),
            }));
            acts
        } else {
            Vec::new()
        };
        diagnostics.push(Diagnostic::with_actions(issue, DESCRIPTION, LABEL, actions));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-rules-sets-dh-compat",
    tags: ["debian-rules-sets-DH_COMPAT"],
    after: ["declares-possibly-conflicting-debhelper-compat-versions"],
    triggers: [debian_workspace::Trigger::File("debian/rules")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version.clone()),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_rules(base: &Path, contents: &str) -> PathBuf {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("rules");
        fs::write(&path, contents).unwrap();
        path
    }

    fn write_control(base: &Path, contents: &str) -> PathBuf {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("control");
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_removes_and_migrates_to_build_depends() {
        let tmp = TempDir::new().unwrap();
        let rules = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nexport DH_COMPAT=10\n\n%:\n\tdh $@\n",
        );
        let control = write_control(
            tmp.path(),
            "Source: foo\nBuild-Depends: pkg-config\n\nPackage: foo\nArchitecture: any\nDescription: x\n y\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, LABEL);
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n\n%:\n\tdh $@\n",
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: foo\nBuild-Depends: debhelper-compat (= 10), pkg-config\n\nPackage: foo\nArchitecture: any\nDescription: x\n y\n",
        );
    }

    #[test]
    fn test_removes_when_matching_compat_file_exists() {
        let tmp = TempDir::new().unwrap();
        let rules = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nexport DH_COMPAT=10\n\n%:\n\tdh $@\n",
        );
        let debian = tmp.path().join("debian");
        fs::write(debian.join("compat"), "10\n").unwrap();
        let control = write_control(
            tmp.path(),
            "Source: foo\nBuild-Depends: debhelper (>= 10)\n\nPackage: foo\nArchitecture: any\nDescription: x\n y\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n\n%:\n\tdh $@\n",
        );
        // compat file should not be overwritten
        assert_eq!(fs::read_to_string(debian.join("compat")).unwrap(), "10\n");
        // control should not get a duplicate debhelper-compat added
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: foo\nBuild-Depends: debhelper (>= 10)\n\nPackage: foo\nArchitecture: any\nDescription: x\n y\n",
        );
    }

    #[test]
    fn test_removes_when_matching_debhelper_compat_build_dep() {
        let tmp = TempDir::new().unwrap();
        let rules = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nexport DH_COMPAT=13\n\n%:\n\tdh $@\n",
        );
        write_control(
            tmp.path(),
            "Source: foo\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: any\nDescription: x\n y\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_leaves_conflict_to_other_fixer() {
        // DH_COMPAT disagrees with debhelper-compat in control:
        // don't touch it; the declares-possibly-conflicting fixer owns
        // that case.
        let tmp = TempDir::new().unwrap();
        let original = "#!/usr/bin/make -f\n\nexport DH_COMPAT=10\n\n%:\n\tdh $@\n";
        let rules = write_rules(tmp.path(), original);
        write_control(
            tmp.path(),
            "Source: foo\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: any\nDescription: x\n y\n",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&rules).unwrap(), original);
    }

    #[test]
    fn test_conditional_operator_not_flagged() {
        let tmp = TempDir::new().unwrap();
        let original = "#!/usr/bin/make -f\n\nDH_COMPAT ?= 10\n\n%:\n\tdh $@\n";
        let rules = write_rules(tmp.path(), original);

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&rules).unwrap(), original);
    }

    #[test]
    fn test_assignment_after_policy_target_not_flagged() {
        let tmp = TempDir::new().unwrap();
        let original =
            "#!/usr/bin/make -f\n\nbuild:\n\tdh_auto_build\n\nDH_COMPAT = 10\n\n%:\n\tdh $@\n";
        let rules = write_rules(tmp.path(), original);

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&rules).unwrap(), original);
    }

    #[test]
    fn test_no_rules_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_assignment_without_export() {
        let tmp = TempDir::new().unwrap();
        let rules = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nDH_COMPAT=9\n\n%:\n\tdh $@\n",
        );
        let control = write_control(
            tmp.path(),
            "Source: foo\nBuild-Depends: pkg-config\n\nPackage: foo\nArchitecture: any\nDescription: x\n y\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n\n%:\n\tdh $@\n",
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: foo\nBuild-Depends: debhelper-compat (= 9), pkg-config\n\nPackage: foo\nArchitecture: any\nDescription: x\n y\n",
        );
    }

    #[test]
    fn test_non_numeric_value_still_removed() {
        // Non-parseable DH_COMPAT value: still remove the deprecated
        // assignment, but don't attempt to migrate a bogus value into
        // Build-Depends.
        let tmp = TempDir::new().unwrap();
        let rules = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nexport DH_COMPAT=$(SOMEVAR)\n\n%:\n\tdh $@\n",
        );
        let control_content =
            "Source: foo\nBuild-Depends: pkg-config\n\nPackage: foo\nArchitecture: any\nDescription: x\n y\n";
        let control = write_control(tmp.path(), control_content);

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n\n%:\n\tdh $@\n",
        );
        // control unchanged - nothing to migrate
        assert_eq!(fs::read_to_string(&control).unwrap(), control_content);
    }
}
