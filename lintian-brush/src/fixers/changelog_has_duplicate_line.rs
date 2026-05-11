use crate::declare_detector;
use crate::diagnostic::{Action, ChangelogAction, Diagnostic};
use crate::{FixerError, FixerPreferences};
use debian_changelog::iter_changes_by_author;
use debian_workspace::Workspace;
use std::collections::HashMap;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let changelog_rel = PathBuf::from("debian/changelog");
    let changelog = match ws.parsed_changelog() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let Some(first_entry) = changelog.iter().next() else {
        return Ok(Vec::new());
    };
    if first_entry.is_unreleased() != Some(true) {
        return Ok(Vec::new());
    }

    let first_package = first_entry.package();
    let Some(first_version) = first_entry.version() else {
        return Ok(Vec::new());
    };
    let first_version_str = first_version.to_string();

    // Track how many times we've seen each (author, text) key. The first
    // occurrence (count 0) is kept; subsequent ones (count 1, 2, …) are
    // emitted as RemoveBullet actions whose `occurrence` matches their
    // position in the bullet stream.
    let mut counts: HashMap<(Option<String>, String), usize> = HashMap::new();
    let mut diagnostics = Vec::new();
    for change in iter_changes_by_author(&changelog) {
        if change.package() != first_package
            || change.version().map(|v| v.to_string()) != Some(first_version_str.clone())
        {
            continue;
        }
        for bullet in change.split_into_bullets() {
            let author = bullet.author().map(|s| s.to_string());
            let text = bullet.lines().join("\n");
            let key = (author.clone(), text.clone());
            let count = counts.entry(key).or_insert(0);
            if *count > 0 {
                let action = Action::Changelog(ChangelogAction::RemoveBullet {
                    file: changelog_rel.clone(),
                    version: first_version_str.clone(),
                    author,
                    text,
                    occurrence: *count,
                });
                diagnostics.push(Diagnostic::untagged(
                    "Duplicate line in changelog.",
                    "Remove duplicate line from changelog.",
                    vec![action],
                ));
            }
            *count += 1;
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "changelog-has-duplicate-line",
    tags: [],
    triggers: [debian_workspace::Trigger::Changelog(
        debian_workspace::ChangelogAspect::Body,
    )],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_simple_duplicate() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("changelog");
        fs::write(
            &path,
            "blah (5.42+dfsg1-2) UNRELEASED; urgency=medium\n\n  * New upstream release.\n  * Fix day-of-week for changelog entry 4.23-1.\n  * New upstream release.\n\n -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 30 Dec 2019 15:25:35 +0000\n\nblah (5.42+dfsg1-1) unstable; urgency=medium\n\n  * Initial Release.\n  * Initial Release.\n\n -- Somebody <somebody@example.com>  Fri, 25 Jan 2019 00:15:07 +0100\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, "Remove duplicate line from changelog.");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "blah (5.42+dfsg1-2) UNRELEASED; urgency=medium\n\n  * New upstream release.\n  * Fix day-of-week for changelog entry 4.23-1.\n\n -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 30 Dec 2019 15:25:35 +0000\n\nblah (5.42+dfsg1-1) unstable; urgency=medium\n\n  * Initial Release.\n  * Initial Release.\n\n -- Somebody <somebody@example.com>  Fri, 25 Jan 2019 00:15:07 +0100\n",
        );
    }

    #[test]
    fn test_already_released() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("changelog");
        let content = "blah (5.42+dfsg1-2) unstable; urgency=medium\n\n  * New upstream release.\n  * Fix day-of-week for changelog entry 4.23-1.\n  * New upstream release.\n\n -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 30 Dec 2019 15:25:35 +0000\n";
        fs::write(&path, content).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn test_no_duplicates() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            "blah (5.42+dfsg1-2) UNRELEASED; urgency=medium\n\n  * New upstream release.\n  * Fix day-of-week for changelog entry 4.23-1.\n\n -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 30 Dec 2019 15:25:35 +0000\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changelog() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
