use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, LintianOverridesAction, OverrideLineSelector};
use crate::lintian_overrides::LintianOverrides;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;

include!(concat!(env!("OUT_DIR"), "/known_tags.rs"));

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for rel in crate::lintian_overrides::override_files(ws)? {
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let parsed = LintianOverrides::parse(content);
        if !parsed.errors().is_empty() {
            continue;
        }
        let overrides = parsed.ok().unwrap();

        for line in overrides.lines() {
            if line.is_comment() || line.is_empty() {
                continue;
            }
            let Some(tag_token) = line.tag() else {
                continue;
            };
            let tag = tag_token.text();
            if is_known_tag(tag) {
                continue;
            }
            let info = line.info().filter(|s| !s.is_empty());
            let package = line.package_spec().and_then(|s| s.package_name());

            let issue = LintianIssue::source_with_info(
                "alien-tag",
                Visibility::Error,
                vec![tag.to_string()],
            );
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    format!("Remove lintian override for unknown tag {}.", tag),
                    "Remove lintian overrides for unknown tags.",
                    vec![Action::LintianOverrides(LintianOverridesAction::DropLine {
                        file: rel.clone(),
                        selector: OverrideLineSelector {
                            tag: tag.to_string(),
                            info,
                            package,
                        },
                    })],
                )
                // The known-tags list is a snapshot from the lintian installed
                // when this was built; a tag we treat as alien may simply be
                // newer than our data. Stay below Certain so we don't drop a
                // valid override at the default certainty.
                .with_certainty(Certainty::Likely),
            );
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "alien-tag",
    tags: ["alien-tag"],
    triggers: [
        debian_workspace::Trigger::File("debian/source/lintian-overrides"),
        debian_workspace::Trigger::Glob("debian/*.lintian-overrides"),
    ],
    cost: crate::detector::DetectorCost::Filesystem,
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::apply_diagnostics;
    use crate::Version;
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn prefs() -> FixerPreferences {
        FixerPreferences {
            minimum_certainty: Some(Certainty::Likely),
            ..FixerPreferences::default()
        }
    }

    fn run(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let ws = FsWorkspace::new(base.to_path_buf(), Some("test".into()), Some(version));
        let diagnostics = detect(&ws, &prefs())?;
        apply_diagnostics(base, &diagnostics, &prefs())
    }

    #[test]
    fn test_no_override_files() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        assert!(matches!(run(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_known_tag_left_alone() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        // renamed-tag is a real tag; it should not be touched.
        fs::write(&overrides, "foo source: renamed-tag some info\n").unwrap();
        assert!(matches!(run(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(&overrides).unwrap(),
            "foo source: renamed-tag some info\n",
        );
    }

    #[test]
    fn test_remove_alien_tag() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(
            &overrides,
            "no-section-field\nthis-tag-does-not-exist-anywhere extra info\n",
        )
        .unwrap();
        run(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&overrides).unwrap(),
            "no-section-field\n",
        );
    }

    #[test]
    fn test_remove_only_alien_deletes_file() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(&overrides, "this-tag-does-not-exist-anywhere\n").unwrap();
        run(tmp.path()).unwrap();
        assert!(!overrides.exists());
    }

    #[test]
    fn test_not_applied_at_default_certainty() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(&overrides, "this-tag-does-not-exist-anywhere\n").unwrap();

        let version: Version = "1.0".parse().unwrap();
        let ws = FsWorkspace::new(tmp.path().to_path_buf(), Some("test".into()), Some(version));
        let diagnostics = detect(&ws, &prefs()).unwrap();
        assert_eq!(diagnostics.len(), 1);
        // The CLI default minimum certainty is Certain, so the fix is held
        // back unless the user opts into less certain changes.
        let strict = FixerPreferences {
            minimum_certainty: Some(Certainty::Certain),
            ..FixerPreferences::default()
        };
        let result = apply_diagnostics(tmp.path(), &diagnostics, &strict);
        assert!(matches!(result, Err(FixerError::NotCertainEnough(..))));
        assert_eq!(
            fs::read_to_string(&overrides).unwrap(),
            "this-tag-does-not-exist-anywhere\n",
        );
    }

    #[test]
    fn test_comments_left_alone() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        // The comment mentions an unknown tag name but is not an override.
        fs::write(
            &overrides,
            "# this-tag-does-not-exist-anywhere is mentioned here\nalien-tag\n",
        )
        .unwrap();
        assert!(matches!(run(tmp.path()), Err(FixerError::NoChanges)));
    }
}
