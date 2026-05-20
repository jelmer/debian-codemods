use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn Workspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    // Skip when not opinionated and the source tree is just the debian/
    // overlay (no upstream contents). See
    // https://salsa.debian.org/debian-ayatana-team/snapd-glib/-/merge_requests/6#note_358358.
    if !preferences.opinionated.unwrap_or(false) {
        if let Some(entries) = ws.list_dir(Path::new(""))? {
            if entries.len() == 1 && entries[0] == "debian" {
                return Ok(Vec::new());
            }
        }
    }

    let rules_rel = PathBuf::from("debian/rules");
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let Some(rule) = makefile
        .rules()
        .find(|r| r.targets().any(|t| t.trim() == "get-orig-source"))
    else {
        return Ok(Vec::new());
    };

    let recipes: Vec<String> = rule.recipes().map(|r| r.trim().to_string()).collect();
    let certainty = if recipes.is_empty()
        || recipes
            .iter()
            .all(|cmd| cmd.split_whitespace().next() == Some("uscan"))
    {
        Certainty::Certain
    } else {
        Certainty::Possible
    };

    let issue = LintianIssue::source_with_info(
        "debian-rules-contains-unnecessary-get-orig-source-target",
        Visibility::Info,
        vec!["[debian/rules]".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "debian/rules contains unnecessary get-orig-source target.",
        "Remove unnecessary get-orig-source-target.",
        vec![
            Action::Makefile(MakefileAction::RemoveRule {
                file: rules_rel.clone(),
                target: "get-orig-source".into(),
            }),
            Action::Makefile(MakefileAction::RemovePhonyTarget {
                file: rules_rel,
                target: "get-orig-source".into(),
            }),
        ],
    )
    .with_certainty(certainty)])
}

declare_detector! {
    name: "debian-rules-contains-unnecessary-get-orig-source-target",
    tags: ["debian-rules-contains-unnecessary-get-orig-source-target"],
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

    fn run_apply(
        base: &Path,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, preferences)
        }
    }

    #[test]
    fn test_removes_get_orig_source() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\nget-orig-source:\n\tuscan\n",
        )
        .unwrap();

        run_apply(
            tmp.path(),
            &FixerPreferences {
                opinionated: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n\n",
        );
    }

    #[test]
    fn test_no_change_when_no_get_orig_source() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("rules"), "#!/usr/bin/make -f\n\n%:\n\tdh $@\n").unwrap();
        assert!(matches!(
            run_apply(tmp.path(), &FixerPreferences::default()),
            Err(FixerError::NoChanges),
        ));
    }

    #[test]
    fn test_no_change_when_only_debian_dir_not_opinionated() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make -f\n\nget-orig-source:\n\tuscan\n",
        )
        .unwrap();
        assert!(matches!(
            run_apply(
                tmp.path(),
                &FixerPreferences {
                    opinionated: Some(false),
                    ..Default::default()
                },
            ),
            Err(FixerError::NoChanges),
        ));
    }
}
