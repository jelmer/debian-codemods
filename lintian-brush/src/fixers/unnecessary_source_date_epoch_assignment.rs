use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use lazy_static::lazy_static;
use regex::Regex;
use std::path::PathBuf;

lazy_static! {
    static ref SHELL_PARSECHANGELOG_RE: Regex = Regex::new(
        r"^\s*\$\(shell\s+dpkg-parsechangelog\s+(-S\s*[Tt]imestamp|--show-field(=|\s+)[Tt]imestamp)\s*\)\s*$",
    )
    .unwrap();
}

/// Whether the assignment's value derives the timestamp from the changelog
/// via `dpkg-parsechangelog`, which is exactly what dpkg does automatically
/// when `SOURCE_DATE_EPOCH` is unset.
fn value_is_dpkg_default(raw_value: &str) -> bool {
    SHELL_PARSECHANGELOG_RE.is_match(raw_value.trim())
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

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for var_def in makefile.variable_definitions() {
        let Some(name) = var_def.name() else {
            continue;
        };
        if name != "SOURCE_DATE_EPOCH" {
            continue;
        }
        let Some(raw_value) = var_def.raw_value() else {
            continue;
        };
        if !value_is_dpkg_default(&raw_value) {
            continue;
        }
        let line_no = var_def.line() + 1;
        let issue = LintianIssue::source_with_info(
            "unnecessary-source-date-epoch-assignment",
            Visibility::Info,
            vec![format!("[debian/rules:{}]", line_no)],
        );
        let action = Action::Makefile(MakefileAction::RemoveVariable {
            file: rules_rel.clone(),
            name: name.clone(),
        });
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "debian/rules assigns SOURCE_DATE_EPOCH unnecessarily.",
            "Drop unnecessary SOURCE_DATE_EPOCH assignment; dpkg sets it automatically.",
            vec![action],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "unnecessary-source-date-epoch-assignment",
    tags: ["unnecessary-source-date-epoch-assignment"],
    triggers: [debian_workspace::Trigger::File("debian/rules")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
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

    #[test]
    fn dpkg_default_recognised() {
        assert!(value_is_dpkg_default(
            "$(shell dpkg-parsechangelog -STimestamp)"
        ));
        assert!(value_is_dpkg_default(
            "$(shell dpkg-parsechangelog -S Timestamp)"
        ));
        assert!(value_is_dpkg_default(
            "$(shell dpkg-parsechangelog --show-field=Timestamp)"
        ));
        assert!(value_is_dpkg_default(
            "$(shell dpkg-parsechangelog --show-field Timestamp)"
        ));
    }

    #[test]
    fn other_values_not_recognised() {
        assert!(!value_is_dpkg_default("1234567890"));
        assert!(!value_is_dpkg_default(
            "$(shell dpkg-parsechangelog -SVersion)"
        ));
        assert!(!value_is_dpkg_default("$(shell date +%s)"));
    }

    #[test]
    fn test_no_rules() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_removes_dpkg_parsechangelog_assignment() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\nexport SOURCE_DATE_EPOCH := $(shell dpkg-parsechangelog -STimestamp)\n\n%:\n\tdh $@\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        let content = fs::read_to_string(&rules).unwrap();
        assert!(!content.contains("SOURCE_DATE_EPOCH"));
    }

    #[test]
    fn test_leaves_specific_timestamp_alone() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make -f\n\nexport SOURCE_DATE_EPOCH = 1234567890\n\n%:\n\tdh $@\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_assignment() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("rules"), "#!/usr/bin/make -f\n\n%:\n\tdh $@\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
