use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, LintianOverridesAction};
use crate::lintian_overrides::LintianOverrides;
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/renamed_tags.rs"));

fn find_override_files(ws: &dyn FixerWorkspace) -> Result<Vec<PathBuf>, FixerError> {
    let mut paths = Vec::new();
    let source_rel = PathBuf::from("debian/source/lintian-overrides");
    if ws.read_file(&source_rel)?.is_some() {
        paths.push(source_rel);
    }
    if let Some(mut entries) = ws.list_dir(Path::new("debian"))? {
        entries.sort();
        for name in entries {
            if name.ends_with(".lintian-overrides") {
                paths.push(PathBuf::from("debian").join(name));
            }
        }
    }
    Ok(paths)
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let renames = get_renamed_tags();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for rel in find_override_files(ws)? {
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(content) = String::from_utf8(bytes) else {
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
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
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
