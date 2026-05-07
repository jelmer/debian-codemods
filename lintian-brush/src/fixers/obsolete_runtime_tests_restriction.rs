use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use deb822_lossless::Deb822;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Read the list of obsolete restrictions from lintian's data file.
fn read_obsolete_restrictions(
    lintian_data_path: Option<&Path>,
) -> Result<HashSet<String>, FixerError> {
    let default_path = PathBuf::from("/usr/share/lintian/data");
    let lintian_data_path = lintian_data_path.unwrap_or(&default_path);

    let path = lintian_data_path
        .join("testsuite")
        .join("known-obsolete-restrictions");
    if !path.exists() {
        return Err(FixerError::Other("Lintian data file not found".to_string()));
    }
    let content = std::fs::read_to_string(&path)?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect())
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/tests/control");
    let bytes = match ws.read_file(&control_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };

    let deprecated = read_obsolete_restrictions(preferences.lintian_data_path.as_deref())?;
    let content = String::from_utf8(bytes)
        .map_err(|e| FixerError::Other(format!("debian/tests/control is not UTF-8: {}", e)))?;
    let parsed = Deb822::parse(&content);
    let deb822 = parsed.tree();

    let mut diagnostics = Vec::new();

    for paragraph in deb822.paragraphs() {
        let Some(restrictions_entry) = paragraph
            .entries()
            .find(|e| e.key().as_deref() == Some("Restrictions"))
        else {
            continue;
        };
        let restrictions_value = restrictions_entry.value();
        let line_num = restrictions_entry.line() + 1;

        let restrictions: Vec<String> = restrictions_value
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if restrictions.is_empty() {
            continue;
        }

        let to_drop: Vec<String> = restrictions
            .iter()
            .filter(|r| deprecated.contains(*r))
            .cloned()
            .collect();
        if to_drop.is_empty() {
            continue;
        }

        let kept: Vec<String> = restrictions
            .iter()
            .filter(|r| !deprecated.contains(*r))
            .cloned()
            .collect();

        // Identify the paragraph by its Tests field, which uniquely names
        // the autopkgtest entry.
        let Some(tests_value) = paragraph.get("Tests") else {
            continue;
        };
        let selector = ParagraphSelector::ByKey {
            field: "Tests".into(),
            value: tests_value,
        };

        // The action set is shared across the per-restriction diagnostics
        // for this paragraph: applying any one is enough; the rest are
        // no-ops because the field already has the kept value (or is gone).
        let action = if kept.is_empty() {
            Action::Deb822(Deb822Action::RemoveField {
                file: control_rel.clone(),
                paragraph: selector.clone(),
                field: "Restrictions".into(),
            })
        } else {
            Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: selector.clone(),
                field: "Restrictions".into(),
                value: kept.join(", "),
            })
        };

        for restriction in &to_drop {
            // needs-recommends is borderline — different lintian
            // versions disagree on it.
            let certainty = if restriction == "needs-recommends" {
                Certainty::Possible
            } else {
                Certainty::Certain
            };
            let issue = LintianIssue::source_with_info(
                "obsolete-runtime-tests-restriction",
                vec![format!(
                    "{} [debian/tests/control:{}]",
                    restriction, line_num
                )],
            );
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    format!(
                        "Obsolete restriction {} in debian/tests/control.",
                        restriction
                    ),
                    format!("Drop deprecated restriction {}.", restriction),
                    vec![action.clone()],
                )
                .with_certainty(certainty),
            );
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let dropped: Vec<String> = fixed
        .iter()
        .filter_map(|(d, _)| {
            let info = d.issue.as_ref()?.info.as_deref()?;
            // info is formatted as "<restriction> [debian/tests/control:<line>]".
            let restriction = info.split_once(" [").map(|(r, _)| r).unwrap_or(info);
            Some(restriction.to_string())
        })
        .collect();
    let plural = if dropped.len() > 1 { "s" } else { "" };
    format!(
        "Drop deprecated restriction{} {}. See https://salsa.debian.org/ci-team/autopkgtest/tree/master/doc/README.package-tests.rst",
        plural,
        dropped.join(", "),
    )
}

declare_detector! {
    name: "obsolete-runtime-tests-restriction",
    tags: ["obsolete-runtime-tests-restriction"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/tests/control",
            paragraph_key: "Tests",
            field: "Restrictions",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/tests/control",
            paragraph_key: "Test-Command",
            field: "Restrictions",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply_with(
        base: &Path,
        prefs: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, prefs)
    }

    fn write_obsolete_data(tmp: &TempDir, contents: &str) -> PathBuf {
        let dir = tmp.path().join("lintian-data/testsuite");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("known-obsolete-restrictions"), contents).unwrap();
        tmp.path().join("lintian-data")
    }

    #[test]
    fn test_remove_obsolete_restriction() {
        let tmp = TempDir::new().unwrap();
        let tests = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests).unwrap();
        let path = tests.join("control");
        fs::write(
            &path,
            "Tests: test1\nRestrictions: needs-root, rw-build-tree\nDepends: @\n\nTests: test2\nRestrictions: breaks-testbed\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            lintian_data_path: Some(write_obsolete_data(&tmp, "rw-build-tree\n")),
            ..Default::default()
        };
        let result = run_apply_with(tmp.path(), &prefs).unwrap();
        assert!(result.description.contains("rw-build-tree"));
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Tests: test1\nRestrictions: needs-root\nDepends: @\n\nTests: test2\nRestrictions: breaks-testbed\n",
        );
    }

    #[test]
    fn test_remove_all_restrictions() {
        let tmp = TempDir::new().unwrap();
        let tests = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests).unwrap();
        let path = tests.join("control");
        fs::write(
            &path,
            "Tests: test1\nRestrictions: rw-build-tree\nDepends: @\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            lintian_data_path: Some(write_obsolete_data(&tmp, "rw-build-tree\n")),
            ..Default::default()
        };
        run_apply_with(tmp.path(), &prefs).unwrap();

        // The Restrictions field is removed entirely; the rest of the
        // paragraph is preserved.
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Tests: test1\nDepends: @\n",
        );
    }

    #[test]
    fn test_needs_recommends_certainty() {
        let tmp = TempDir::new().unwrap();
        let tests = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests).unwrap();
        let path = tests.join("control");
        fs::write(&path, "Tests: test1\nRestrictions: needs-recommends\n").unwrap();

        let prefs = FixerPreferences {
            lintian_data_path: Some(write_obsolete_data(&tmp, "needs-recommends\n")),
            ..Default::default()
        };
        let result = run_apply_with(tmp.path(), &prefs).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Possible));
    }

    #[test]
    fn test_no_changes_when_no_obsolete() {
        let tmp = TempDir::new().unwrap();
        let tests = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests).unwrap();
        let path = tests.join("control");
        let original = "Tests: test1\nRestrictions: needs-root\n";
        fs::write(&path, original).unwrap();

        let prefs = FixerPreferences {
            lintian_data_path: Some(write_obsolete_data(&tmp, "rw-build-tree\n")),
            ..Default::default()
        };
        assert!(matches!(
            run_apply_with(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
}
