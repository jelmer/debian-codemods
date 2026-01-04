use crate::{declare_fixer, FixerError, FixerResult};
use debian_changelog::iter_changes_by_author;
use debian_changelog::ChangeLog;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let changelog_path = base_path.join("debian/changelog");

    if !changelog_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&changelog_path)?;
    let changelog: ChangeLog = content.parse()?;

    // Get the first (topmost) entry
    let mut entries = changelog.iter();
    let first_entry = if let Some(e) = entries.next() {
        e
    } else {
        return Err(FixerError::NoChanges);
    };

    // Only process UNRELEASED entries
    if first_entry.is_unreleased() != Some(true) {
        return Err(FixerError::NoChanges);
    }

    // Get all changes from the changelog
    let all_changes = iter_changes_by_author(&changelog);

    let first_entry_package = first_entry.package();
    let first_entry_version = first_entry.version();

    let mut seen: HashSet<(Option<String>, String)> = HashSet::new();
    let mut made_changes = false;

    for change in all_changes.into_iter() {
        // Only process changes from the first entry
        if change.package() == first_entry_package && change.version() == first_entry_version {
            // Split this change into individual bullet points
            let bullets = change.split_into_bullets();

            for bullet in bullets {
                let author = bullet.author().map(|s| s.to_string());
                let bullet_text = bullet.lines().join("\n");
                let key = (author, bullet_text);

                if !seen.insert(key) {
                    // insert() returns false if the key was already present - it's a duplicate
                    bullet.remove();
                    made_changes = true;
                }
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    // Write the updated changelog
    let new_content = changelog.to_string();
    fs::write(&changelog_path, new_content)?;

    Ok(FixerResult::builder("Remove duplicate line from changelog.").build())
}

declare_fixer! {
    name: "changelog-has-duplicate-line",
    tags: [],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_simple_duplicate() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = "blah (5.42+dfsg1-2) UNRELEASED; urgency=medium\n\n  * New upstream release.\n  * Fix day-of-week for changelog entry 4.23-1.\n  * New upstream release.\n\n -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 30 Dec 2019 15:25:35 +0000\n\nblah (5.42+dfsg1-1) unstable; urgency=medium\n\n  * Initial Release.\n  * Initial Release.\n\n -- Somebody <somebody@example.com>  Fri, 25 Jan 2019 00:15:07 +0100\n";
        fs::write(&changelog_path, content).unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove duplicate line from changelog.");

        let new_content = fs::read_to_string(&changelog_path).unwrap();

        // Check that the duplicate in the UNRELEASED section was removed
        // Parse to get just the first entry (up to the signature line)
        let first_entry_text: Vec<&str> = new_content
            .lines()
            .take_while(|l| !l.starts_with("blah (5.42+dfsg1-1)"))
            .filter(|l| l.trim().starts_with("*"))
            .collect();

        assert_eq!(first_entry_text.len(), 2); // Should have only 2 entries, not 3

        // The released section should remain unchanged
        assert!(new_content.contains("blah (5.42+dfsg1-1) unstable"));
    }

    #[test]
    fn test_already_released() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = "blah (5.42+dfsg1-2) unstable; urgency=medium\n\n  * New upstream release.\n  * Fix day-of-week for changelog entry 4.23-1.\n  * New upstream release.\n\n -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 30 Dec 2019 15:25:35 +0000\n";
        fs::write(&changelog_path, content).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Content should be unchanged
        let new_content = fs::read_to_string(&changelog_path).unwrap();
        assert_eq!(new_content, content);
    }

    #[test]
    fn test_no_duplicates() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = "blah (5.42+dfsg1-2) UNRELEASED; urgency=medium\n\n  * New upstream release.\n  * Fix day-of-week for changelog entry 4.23-1.\n\n -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 30 Dec 2019 15:25:35 +0000\n";
        fs::write(&changelog_path, content).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changelog() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
