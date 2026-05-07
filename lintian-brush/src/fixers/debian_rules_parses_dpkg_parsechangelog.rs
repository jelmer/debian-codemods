use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::path::PathBuf;

const PKG_INFO_PATH: &str = "/usr/share/dpkg/pkg-info.mk";

lazy_static! {
    static ref KNOWN_COMMANDS: Vec<(&'static str, &'static str)> = vec![(
        "dpkg-parsechangelog | sed -n -e 's/^Version: //p'",
        "DEB_VERSION"
    ),];
    static ref VAR_RE: Regex = Regex::new(r"([A-Z_]+)\s*([:?]?=)\s*(.*)").unwrap();
    static ref SHELL_RE: Regex = Regex::new(r"\$\(shell\s+(.*)\)").unwrap();
}

fn load_pkg_info_variables() -> HashSet<String> {
    let mut variables = HashSet::new();
    if let Ok(content) = std::fs::read_to_string(PKG_INFO_PATH) {
        for line in content.lines() {
            if let Some(caps) = VAR_RE.captures(line.trim()) {
                if let Some(var) = caps.get(1) {
                    variables.insert(var.as_str().to_string());
                }
            }
        }
    }
    variables
}

fn matches_known_command(value: &str, expected_var: &str) -> bool {
    let Some(caps) = SHELL_RE.captures(value.trim()) else {
        return false;
    };
    let Some(cmd) = caps.get(1) else {
        return false;
    };
    let cmd_str = cmd.as_str().trim();
    KNOWN_COMMANDS
        .iter()
        .any(|(known_cmd, known_var)| cmd_str == *known_cmd && expected_var == *known_var)
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let pkg_info_vars = load_pkg_info_variables();
    let already_included = makefile.included_files().any(|f| f == PKG_INFO_PATH);

    let mut issues: Vec<LintianIssue> = Vec::new();
    let mut actions: Vec<Action> = Vec::new();
    for var_def in makefile.variable_definitions() {
        let Some(name) = var_def.name() else {
            continue;
        };
        let Some(value) = var_def.raw_value() else {
            continue;
        };
        if !pkg_info_vars.contains(&name) {
            continue;
        }
        if !matches_known_command(&value, &name) {
            continue;
        }
        issues.push(LintianIssue::source_with_info(
            "debian-rules-parses-dpkg-parsechangelog",
            vec![format!("{} [debian/rules]", name)],
        ));
        actions.push(Action::Makefile(MakefileAction::RemoveVariable {
            file: rules_rel.clone(),
            name,
        }));
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    if !already_included {
        actions.insert(
            0,
            Action::Makefile(MakefileAction::AddInclude {
                file: rules_rel.clone(),
                path: PKG_INFO_PATH.to_string(),
            }),
        );
    }

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for (i, issue) in issues.into_iter().enumerate() {
        let plan_actions = if i == 0 { actions.clone() } else { Vec::new() };
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "debian/rules invokes dpkg-parsechangelog directly.",
            "Avoid invoking dpkg-parsechangelog.",
            plan_actions,
        ));
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "debian-rules-parses-dpkg-parsechangelog",
    tags: ["debian-rules-parses-dpkg-parsechangelog"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_no_rules() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_replaces_dpkg_parsechangelog() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\nDEB_VERSION := $(shell dpkg-parsechangelog | sed -n -e 's/^Version: //p')\nDEB_UPSTREAM_VERSION := $(shell echo $(DEB_VERSION) | cut -d+ -f1)\n\n%:\n\tdh $@\n\nversion:\n\techo $(DEB_VERSION)\n",
        )
        .unwrap();

        if !std::path::Path::new(PKG_INFO_PATH).exists() {
            // Without pkg-info.mk on the system we can't load the
            // variable list; the fixer correctly bails to NoChanges.
            assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
            return;
        }

        run_apply(tmp.path()).unwrap();
        let content = fs::read_to_string(&rules).unwrap();
        assert!(content.contains("include /usr/share/dpkg/pkg-info.mk"));
        assert!(!content.contains("dpkg-parsechangelog"));
        assert!(content.contains("DEB_UPSTREAM_VERSION"));
    }
}
