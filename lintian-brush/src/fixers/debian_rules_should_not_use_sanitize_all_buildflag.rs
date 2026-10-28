use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const VAR_NAME: &str = "DEB_BUILD_MAINT_OPTIONS";
const SANITIZE_TOKEN: &str = "sanitize=+all";

const DESCRIPTION: &str = "debian/rules enables sanitize=+all via DEB_BUILD_MAINT_OPTIONS.";
const LABEL: &str = "Drop sanitize=+all from DEB_BUILD_MAINT_OPTIONS in debian/rules.";

/// Remove the `sanitize=+all` token from a DEB_BUILD_MAINT_OPTIONS value,
/// preserving the surrounding whitespace style where possible. Returns the
/// trimmed-remainder value or `None` if nothing other than the sanitize token
/// (and whitespace) remains.
fn strip_sanitize_all(raw_value: &str) -> Option<String> {
    let tokens: Vec<&str> = raw_value.split_whitespace().collect();
    let remaining: Vec<&str> = tokens
        .into_iter()
        .filter(|t| *t != SANITIZE_TOKEN)
        .collect();
    if remaining.is_empty() {
        None
    } else {
        Some(remaining.join(" "))
    }
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut diagnostics = Vec::new();
    for var in makefile.find_variable(VAR_NAME) {
        // Lintian's pattern matches `=` and `:=` but not `?=` or `+=`.
        if !matches!(var.assignment_operator().as_deref(), Some("=") | Some(":=")) {
            continue;
        }
        let Some(raw_value) = var.raw_value() else {
            continue;
        };
        // Match lintian's `\bsanitize=\+all\b` test on the raw value, so we only
        // report when the token is actually present.
        if !raw_value.split_whitespace().any(|t| t == SANITIZE_TOKEN) {
            continue;
        }

        let line_no = var.line() + 1;
        let issue = LintianIssue::source_with_info(
            "debian-rules-should-not-use-sanitize-all-buildflag",
            Visibility::Error,
            vec![format!("[debian/rules:{}]", line_no)],
        );

        let action = match strip_sanitize_all(&raw_value) {
            Some(new_value) => Action::Makefile(MakefileAction::SetVariable {
                file: rules_rel.clone(),
                name: VAR_NAME.to_string(),
                value: new_value,
            }),
            None => Action::Makefile(MakefileAction::RemoveVariable {
                file: rules_rel.clone(),
                name: VAR_NAME.to_string(),
            }),
        };

        diagnostics.push(Diagnostic::with_actions(
            issue,
            DESCRIPTION,
            LABEL,
            vec![action],
        ));
        // The applier only acts on the first matching variable, so one
        // diagnostic covers the fixable case.
        break;
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-rules-should-not-use-sanitize-all-buildflag",
    tags: ["debian-rules-should-not-use-sanitize-all-buildflag"],
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
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version.clone()),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_rules(base: &Path, contents: &str) -> PathBuf {
        let debian = base.join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("rules");
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn strip_sanitize_all_removes_sole_token() {
        assert_eq!(strip_sanitize_all("sanitize=+all"), None);
    }

    #[test]
    fn strip_sanitize_all_preserves_other_tokens() {
        assert_eq!(
            strip_sanitize_all("hardening=+all sanitize=+all"),
            Some("hardening=+all".to_string())
        );
        assert_eq!(
            strip_sanitize_all("sanitize=+all hardening=+all"),
            Some("hardening=+all".to_string())
        );
        assert_eq!(
            strip_sanitize_all("hardening=+all sanitize=+all qa=+bug"),
            Some("hardening=+all qa=+bug".to_string())
        );
    }

    #[test]
    fn strip_sanitize_all_leaves_unrelated_sanitize_values_alone() {
        // Only the exact `sanitize=+all` token is stripped; a `sanitize=+address`
        // or similar partial opt-in is kept.
        assert_eq!(
            strip_sanitize_all("sanitize=+address"),
            Some("sanitize=+address".to_string())
        );
    }

    #[test]
    fn test_sole_sanitize_all_removes_variable() {
        let tmp = TempDir::new().unwrap();
        let path = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nexport DEB_BUILD_MAINT_OPTIONS = sanitize=+all\n\n%:\n\tdh $@\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_sanitize_all_mixed_with_hardening() {
        let tmp = TempDir::new().unwrap();
        let path = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nexport DEB_BUILD_MAINT_OPTIONS = hardening=+all sanitize=+all\n\n%:\n\tdh $@\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\nexport DEB_BUILD_MAINT_OPTIONS = hardening=+all\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_colon_equals_also_fixed() {
        let tmp = TempDir::new().unwrap();
        let path = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nDEB_BUILD_MAINT_OPTIONS := sanitize=+all hardening=+all\n\n%:\n\tdh $@\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\nDEB_BUILD_MAINT_OPTIONS := hardening=+all\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_conditional_assignment_not_touched() {
        // Lintian's `\s*:?=` does not match `?=`, so a defensive default is
        // left alone.
        let tmp = TempDir::new().unwrap();
        let original =
            "#!/usr/bin/make -f\n\nDEB_BUILD_MAINT_OPTIONS ?= sanitize=+all\n\n%:\n\tdh $@\n";
        let path = write_rules(tmp.path(), original);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_append_operator_not_touched() {
        let tmp = TempDir::new().unwrap();
        let original =
            "#!/usr/bin/make -f\n\nDEB_BUILD_MAINT_OPTIONS += sanitize=+all\n\n%:\n\tdh $@\n";
        let path = write_rules(tmp.path(), original);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_sanitize_all_no_changes() {
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

    #[test]
    fn test_partial_sanitize_token_not_matched() {
        // `sanitize=+address` is a distinct option and must not be stripped
        // when a user has explicitly opted into a single sanitizer.
        let tmp = TempDir::new().unwrap();
        let original =
            "#!/usr/bin/make -f\n\nexport DEB_BUILD_MAINT_OPTIONS = sanitize=+address\n\n%:\n\tdh $@\n";
        write_rules(tmp.path(), original);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
