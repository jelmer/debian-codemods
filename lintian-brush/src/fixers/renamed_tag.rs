use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, LintianOverridesAction};
use crate::lintian_overrides::LintianOverrides;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;

include!(concat!(env!("OUT_DIR"), "/renamed_tags.rs"));

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let renames = get_renamed_tags();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for rel in crate::lintian_overrides::override_files(ws)? {
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let parsed = LintianOverrides::parse(&content);
        if !parsed.errors().is_empty() {
            continue;
        }
        let overrides = parsed.ok().unwrap();

        for line in overrides.lines() {
            let Some(tag_token) = line.tag() else {
                continue;
            };
            let Some(new_tag) = renames.get(tag_token.text()) else {
                continue;
            };
            let issue = LintianIssue::source_with_info(
                "renamed-tag",
                Visibility::Pedantic,
                vec![format!("{} => {}", tag_token.text(), new_tag)],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                "Lintian override uses renamed tag name.",
                "Update renamed lintian tag names in lintian overrides.",
                vec![Action::LintianOverrides(
                    LintianOverridesAction::RenameTag {
                        file: rel.clone(),
                        from_tag: tag_token.text().to_string(),
                        to_tag: (*new_tag).to_string(),
                    },
                )],
            ));
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "renamed-tag",
    tags: ["renamed-tag"],
    triggers: [
        debian_workspace::Trigger::File("debian/source/lintian-overrides"),
        debian_workspace::Trigger::Glob("debian/*.lintian-overrides"),
    ],
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
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(v.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_no_override_files() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_renames_needed() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let overrides = debian.join("lintian-overrides");
        fs::write(
            &overrides,
            "# Comment line\nsource-package-name: some-current-tag some info\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(&overrides).unwrap(),
            "# Comment line\nsource-package-name: some-current-tag some info\n",
        );
    }

    #[test]
    fn test_rename_tags() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let overrides = debian.join("test-package.lintian-overrides");
        fs::write(
            &overrides,
            "# Comment\nsource-package: debian-changelog-has-wrong-weekday some info\nbinary-package: binary-without-manpage\n",
        )
        .unwrap();
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&overrides).unwrap(),
            "# Comment\nsource-package: debian-changelog-has-wrong-day-of-week some info\nbinary-package: no-manual-page\n",
        );
    }

    #[test]
    fn test_source_overrides() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(&overrides, "debian-changelog-has-wrong-weekday\n").unwrap();
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&overrides).unwrap(),
            "debian-changelog-has-wrong-day-of-week\n",
        );
    }
}
