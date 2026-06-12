use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const OLD_NAME: &str = "DEB_BUILD_OPTIONS";
const NEW_NAME: &str = "DEB_BUILD_MAINT_OPTIONS";

const DESCRIPTION: &str = "debian/rules sets DEB_BUILD_OPTIONS instead of DEB_BUILD_MAINT_OPTIONS.";
const LABEL: &str = "Set DEB_BUILD_MAINT_OPTIONS rather than DEB_BUILD_OPTIONS in debian/rules.";

/// The policy-mandated targets lintian tracks. The DEB_BUILD_OPTIONS tag is
/// only emitted for assignments that appear before any of these targets is
/// defined (lintian's `unless (%seen)` guard).
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

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    // The first line at which a policy target is introduced, either as a
    // rule target or as a `.PHONY` prerequisite. Assignments at or after
    // this line are not flagged (lintian's `unless (%seen)` guard).
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

    let mut issues = Vec::new();
    for var in makefile.find_variable(OLD_NAME) {
        // lintian's pattern is `\s*:?=`, so it matches `=` and `:=` but not
        // `?=` or `+=`; a defensive `?=` default is left alone.
        if !matches!(var.assignment_operator().as_deref(), Some("=") | Some(":=")) {
            continue;
        }
        if boundary.is_some_and(|b| var.line() >= b) {
            continue;
        }
        let line_no = var.line() + 1;
        issues.push(LintianIssue::source_with_info(
            "debian-rules-sets-DEB_BUILD_OPTIONS",
            Visibility::Warning,
            vec![format!("[debian/rules:{}]", line_no)],
        ));
    }

    if issues.is_empty() {
        return Ok(Vec::new());
    }

    // A single rename targets the first matching definition; that is enough
    // for the common case of one top-level assignment.
    let action = Action::Makefile(MakefileAction::RenameVariable {
        file: PathBuf::from("debian/rules"),
        from_name: OLD_NAME.to_string(),
        to_name: NEW_NAME.to_string(),
    });

    let mut diagnostics = Vec::new();
    for (i, issue) in issues.into_iter().enumerate() {
        let actions = if i == 0 {
            vec![action.clone()]
        } else {
            Vec::new()
        };
        diagnostics.push(Diagnostic::with_actions(issue, DESCRIPTION, LABEL, actions));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-rules-sets-deb-build-options",
    tags: ["debian-rules-sets-DEB_BUILD_OPTIONS"],
    triggers: [debian_workspace::Trigger::File("debian/rules")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    fn write_rules(base: &Path, contents: &str) -> PathBuf {
        let debian = base.join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("rules");
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_simple_replacement() {
        let tmp = TempDir::new().unwrap();
        let path = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nDEB_BUILD_OPTIONS := nocheck\n\n%:\n\tdh $@\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, LABEL);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\nDEB_BUILD_MAINT_OPTIONS := nocheck\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_export_form() {
        let tmp = TempDir::new().unwrap();
        let path = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nexport DEB_BUILD_OPTIONS = nocheck\n\n%:\n\tdh $@\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\nexport DEB_BUILD_MAINT_OPTIONS = nocheck\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_conditional_operator_not_flagged() {
        // lintian's pattern is `\s*:?=`, so it matches `=` and `:=` but not
        // `?=` or `+=`. Matching its behaviour avoids touching a defensive
        // `?=` default.
        let tmp = TempDir::new().unwrap();
        let original = "#!/usr/bin/make -f\n\nDEB_BUILD_OPTIONS ?= nocheck\n\n%:\n\tdh $@\n";
        let path = write_rules(tmp.path(), original);

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_expansion_not_touched() {
        // A `$(DEB_BUILD_OPTIONS)` expansion is a legitimate read, not an
        // assignment, so it must be left alone.
        let tmp = TempDir::new().unwrap();
        let original =
            "#!/usr/bin/make -f\n\nifneq (,$(filter nocheck,$(DEB_BUILD_OPTIONS)))\nexport FOO = 1\nendif\n\n%:\n\tdh $@\n";
        let path = write_rules(tmp.path(), original);

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_assignment_inside_target_not_flagged() {
        // The assignment here is a recipe line under `build:`, not a
        // top-level variable definition, so it is not flagged.
        let tmp = TempDir::new().unwrap();
        let original =
            "#!/usr/bin/make -f\n\nbuild:\n\tDEB_BUILD_OPTIONS=nocheck $(MAKE)\n\n%:\n\tdh $@\n";
        let path = write_rules(tmp.path(), original);

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_assignment_after_policy_target_not_flagged() {
        // lintian only flags assignments before any policy target. A
        // top-level assignment appearing after `build:` is not flagged.
        let tmp = TempDir::new().unwrap();
        let original =
            "#!/usr/bin/make -f\n\nbuild:\n\tdh_auto_build\n\nDEB_BUILD_OPTIONS := nocheck\n\n%:\n\tdh $@\n";
        let path = write_rules(tmp.path(), original);

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_only_name_replaced_value_preserved() {
        // A value that mentions the variable name must not be rewritten;
        // only the assignment target token changes.
        let tmp = TempDir::new().unwrap();
        let path = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nDEB_BUILD_OPTIONS := $(DEB_BUILD_OPTIONS) nocheck\n\n%:\n\tdh $@\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\nDEB_BUILD_MAINT_OPTIONS := $(DEB_BUILD_OPTIONS) nocheck\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_already_uses_maint_options() {
        let tmp = TempDir::new().unwrap();
        let original =
            "#!/usr/bin/make -f\n\nexport DEB_BUILD_MAINT_OPTIONS = hardening=+all\n\n%:\n\tdh $@\n";
        write_rules(tmp.path(), original);

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
