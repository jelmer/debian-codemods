//! Functions for working with debian/changelog files.
use crate::release_info;
use breezyshim::error::Error;
use breezyshim::tree::{Tree, TreeChange, WorkingTree};
use debian_changelog::ChangeLog;

/// Check whether the only change in a tree is to the last changelog entry.
///
/// # Arguments
/// * `tree`: Tree to analyze
/// * `changelog_path`: Path to the changelog file
/// * `changes`: Changes in the tree
pub fn only_changes_last_changelog_block<'a>(
    tree: &WorkingTree,
    basis_tree: &dyn Tree,
    changelog_path: &std::path::Path,
    changes: impl Iterator<Item = &'a TreeChange>,
) -> Result<bool, debian_changelog::Error> {
    let read_lock = tree.lock_read();
    let basis_lock = basis_tree.lock_read();
    let mut changes_seen = false;
    for change in changes {
        if let Some(path) = change.path.1.as_ref() {
            if path == std::path::Path::new("") {
                continue;
            }
            if path == changelog_path {
                changes_seen = true;
                continue;
            }
            if !tree.has_versioned_directories() && changelog_path.starts_with(path) {
                // Directory leading up to changelog
                continue;
            }
        }
        // If the change is not in the changelog, it's not just a changelog change
        return Ok(false);
    }

    if !changes_seen {
        // Doesn't change the changelog at all
        return Ok(false);
    }
    let mut new_cl = match tree.get_file(changelog_path) {
        Ok(f) => ChangeLog::read(f)?,
        Err(Error::NoSuchFile(_)) => {
            return Ok(false);
        }
        Err(e) => {
            panic!("Error reading changelog: {}", e);
        }
    };
    let mut old_cl = match basis_tree.get_file(changelog_path) {
        Ok(f) => ChangeLog::read(f)?,
        Err(Error::NoSuchFile(_)) => {
            return Ok(true);
        }
        Err(e) => {
            panic!("Error reading changelog: {}", e);
        }
    };
    let first_entry = if let Some(e) = new_cl.pop_first() {
        e
    } else {
        // No entries
        return Ok(false);
    };
    if first_entry.distributions().as_deref() != Some(&["UNRELEASED".into()]) {
        // Not unreleased
        return Ok(false);
    }
    old_cl.pop_first();
    std::mem::drop(read_lock);
    std::mem::drop(basis_lock);
    Ok(new_cl.to_string() == old_cl.to_string())
}

/// Find the last distribution the package was uploaded to.
pub fn find_last_distribution(cl: &ChangeLog) -> Option<String> {
    for block in cl.iter() {
        if block.is_unreleased() != Some(true) {
            if let Some(distributions) = block.distributions() {
                if distributions.len() == 1 {
                    return Some(distributions[0].clone());
                }
            }
        }
    }
    None
}

/// Given a tree, find the previous upload to the distribution.
///
/// When e.g. Ubuntu merges from Debian they want to build with
/// -vPREV_VERSION. Here's where we find that previous version.
///
/// We look at the last changelog entry and find the upload target.
/// We then search backwards until we find the same target. That's
/// the previous version that we return.
///
/// We require there to be a previous version, otherwise we throw
/// an error.
///
/// It's not a simple string comparison to find the same target in
/// a previous version, as we should consider old series in e.g.
/// Ubuntu.
pub fn find_previous_upload(changelog: &ChangeLog) -> Option<debversion::Version> {
    let current_target = find_last_distribution(changelog)?;
    // multiple debian pockets with all debian releases
    let all_debian = crate::release_info::debian_releases()
        .iter()
        .flat_map(|r| {
            release_info::DEBIAN_POCKETS
                .iter()
                .map(move |t| format!("{}{}", r, t))
        })
        .collect::<Vec<_>>();
    let all_ubuntu = crate::release_info::ubuntu_releases()
        .iter()
        .flat_map(|r| {
            release_info::UBUNTU_POCKETS
                .iter()
                .map(move |t| format!("{}{}", r, t))
        })
        .collect::<Vec<_>>();
    let match_targets = if all_debian.contains(&current_target) {
        vec![current_target]
    } else if all_ubuntu.contains(&current_target) {
        let mut match_targets = crate::release_info::ubuntu_releases();
        if current_target.contains('-') {
            let distro = current_target.split('-').next().unwrap();
            match_targets.extend(
                release_info::DEBIAN_POCKETS
                    .iter()
                    .map(|r| format!("{}{}", r, distro)),
            );
        }
        match_targets
    } else {
        // If we do not recognize the current target in order to apply special
        // rules to it, then just assume that only previous uploads to exactly
        // the same target count.
        vec![current_target]
    };
    for block in changelog.iter().skip(1) {
        if match_targets.contains(&block.distributions().unwrap()[0]) {
            return block.version().clone();
        }
    }

    None
}

#[derive(Debug)]
/// Error type for find_changelog
pub enum FindChangelogError {
    /// No changelog found in the given files
    MissingChangelog(Vec<std::path::PathBuf>),

    /// Add a changelog at the given file
    AddChangelog(std::path::PathBuf),

    /// Error parsing the changelog
    ChangelogParseError(String),

    /// Error from breezyshim
    BrzError(breezyshim::error::Error),
}

impl std::fmt::Display for FindChangelogError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FindChangelogError::MissingChangelog(files) => {
                write!(f, "No changelog found in {:?}", files)
            }
            FindChangelogError::AddChangelog(file) => {
                write!(f, "Add a changelog at {:?}", file)
            }
            FindChangelogError::ChangelogParseError(e) => write!(f, "{}", e),
            FindChangelogError::BrzError(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for FindChangelogError {}

impl From<breezyshim::error::Error> for FindChangelogError {
    fn from(e: breezyshim::error::Error) -> Self {
        FindChangelogError::BrzError(e)
    }
}

/// Find the changelog in the given tree.
///
/// First looks for 'debian/changelog'. If "merge" is true will also
/// look for 'changelog'.
///
/// The returned changelog is created with 'allow_empty_author=True'
/// as some people do this but still want to build.
/// 'max_blocks' defaults to 1 to try and prevent old broken
/// changelog entries from causing the command to fail.
///
/// "top_level" is a subset of "merge" mode. It indicates that the
/// '.bzr' dir is at the same level as 'changelog' etc., rather
/// than being at the same level as 'debian/'.
///
/// # Arguments
/// * `tree`: Tree to look in
/// * `subpath`: Path to the changelog file
/// * `merge`: Whether this is a "merge" package
///
/// # Returns
/// * (changelog, top_level) where changelog is the Changelog,
///   and top_level is a boolean indicating whether the file is
///   located at 'changelog' (rather than 'debian/changelog') if
///   merge was given, False otherwise.
pub fn find_changelog(
    tree: &dyn Tree,
    subpath: &std::path::Path,
    merge: Option<bool>,
) -> Result<(ChangeLog, bool), FindChangelogError> {
    let mut top_level = false;
    let lock = tree.lock_read();

    let mut changelog_file = subpath.join("debian/changelog");
    if !tree.has_filename(&changelog_file) {
        let mut checked_files = vec![changelog_file.clone()];
        let changelog_file = if merge.unwrap_or(false) {
            // Assume LarstiQ's layout (.bzr in debian/)
            let changelog_file = subpath.join("changelog");
            top_level = true;
            if !tree.has_filename(&changelog_file) {
                checked_files.push(changelog_file);
                None
            } else {
                Some(changelog_file)
            }
        } else {
            None
        };
        if changelog_file.is_none() {
            return Err(FindChangelogError::MissingChangelog(checked_files));
        }
    } else if merge.unwrap_or(true) && tree.has_filename(&subpath.join("changelog")) {
        // If it is a "top_level" package and debian is a symlink to
        // "." then it will have found debian/changelog. Try and detect
        // this.
        let debian_file = subpath.join("debian");
        if tree.is_versioned(&debian_file)
            && tree.kind(&debian_file)? == breezyshim::tree::Kind::Symlink
            && tree.get_symlink_target(&debian_file)?.as_path() == std::path::Path::new(".")
        {
            changelog_file = "changelog".into();
            top_level = true;
        }
    }
    log::debug!(
        "Using '{}' to get package information",
        changelog_file.display()
    );
    if !tree.is_versioned(&changelog_file) {
        return Err(FindChangelogError::AddChangelog(changelog_file));
    }
    let contents = tree.get_file_text(&changelog_file)?;
    std::mem::drop(lock);
    let changelog = ChangeLog::read_relaxed(contents.as_slice()).unwrap();
    Ok((changelog, top_level))
}

#[cfg(test)]
mod tests {
    use super::*;
    pub const COMMITTER: &str = "Test User <example@example.com>";
    #[test]
    fn test_find_previous_upload() {
        let cl = r#"test (1.0-1) unstable; urgency=medium

  * Initial release.

 -- Test User <test@user.example.com>  Fri, 01 Jan 2021 00:00:00 +0000
"#
        .parse()
        .unwrap();
        assert_eq!(super::find_previous_upload(&cl), None);

        let cl = r#"test (1.0-1) unstable; urgency=medium

  * More change.

 -- Test User <test@user.example.com>  Fri, 01 Jan 2021 00:00:00 +0000

test (1.0-0) unstable; urgency=medium

  * Initial release.

 -- Test User <test@example.com>  Fri, 01 Jan 2021 00:00:00 +0000
"#
        .parse()
        .unwrap();
        assert_eq!(
            super::find_previous_upload(&cl),
            Some("1.0-0".parse().unwrap())
        );
    }

    mod test_only_changes_last_changelog_block {
        use super::*;
        use breezyshim::controldir::{create_standalone_workingtree, ControlDirFormat};
        use breezyshim::tree::Path;
        use breezyshim::tree::Tree;
        fn make_package_tree(p: &std::path::Path) -> breezyshim::tree::WorkingTree {
            let tree = create_standalone_workingtree(p, &ControlDirFormat::default()).unwrap();
            std::fs::create_dir_all(p.join("debian")).unwrap();

            std::fs::write(
                p.join("debian/control"),
                r###"Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"###,
            )
            .unwrap();
            std::fs::write(
                p.join("debian/changelog"),
                r###"blah (0.2) UNRELEASED; urgency=medium

  * And a change.

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100

blah (0.1) unstable; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"###,
            )
            .unwrap();
            tree.add(&[
                Path::new("debian"),
                Path::new("debian/changelog"),
                Path::new("debian/control"),
            ])
            .unwrap();
            tree.build_commit()
                .message("Initial thingy.")
                .committer(COMMITTER)
                .commit()
                .unwrap();
            tree
        }

        #[test]
        fn test_no_changes() {
            let td = tempfile::tempdir().unwrap();
            let tree = make_package_tree(td.path());
            let basis_tree = tree.basis_tree().unwrap();
            let lock_read = tree.lock_read();
            let basis_lock_read = basis_tree.lock_read();
            let changes = tree
                .iter_changes(&basis_tree, None, None, None)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(!only_changes_last_changelog_block(
                &tree,
                &tree.basis_tree().unwrap(),
                Path::new("debian/changelog"),
                changes.iter()
            )
            .unwrap());
            std::mem::drop(basis_lock_read);
            std::mem::drop(lock_read);
        }

        #[test]
        fn test_other_change() {
            let td = tempfile::tempdir().unwrap();
            let tree = make_package_tree(td.path());
            std::fs::write(
                td.path().join("debian/control"),
                r###"Source: blah
Vcs-Bzr: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all
"###,
            )
            .unwrap();
            let basis_tree = tree.basis_tree().unwrap();
            let lock_read = tree.lock_read();
            let basis_lock_read = basis_tree.lock_read();
            let changes = tree
                .iter_changes(&basis_tree, None, None, None)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(!only_changes_last_changelog_block(
                &tree,
                &tree.basis_tree().unwrap(),
                Path::new("debian/changelog"),
                changes.iter()
            )
            .unwrap());
            std::mem::drop(basis_lock_read);
            std::mem::drop(lock_read);
        }

        #[test]
        fn test_other_changes() {
            let td = tempfile::tempdir().unwrap();
            let tree = make_package_tree(td.path());
            std::fs::write(
                td.path().join("debian/control"),
                r###"Source: blah
Vcs-Bzr: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"###,
            )
            .unwrap();
            std::fs::write(
                td.path().join("debian/changelog"),
                r###"blah (0.1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #911016)
  * Some other change.

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"###,
            )
            .unwrap();
            let basis_tree = tree.basis_tree().unwrap();
            let lock_read = tree.lock_read();
            let basis_lock_read = basis_tree.lock_read();
            let changes = tree
                .iter_changes(&basis_tree, None, None, None)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(!only_changes_last_changelog_block(
                &tree,
                &tree.basis_tree().unwrap(),
                Path::new("debian/changelog"),
                changes.iter()
            )
            .unwrap());
            std::mem::drop(basis_lock_read);
            std::mem::drop(lock_read);
        }

        #[test]
        fn test_changes_to_other_changelog_entries() {
            let td = tempfile::tempdir().unwrap();
            let tree = make_package_tree(td.path());
            std::fs::write(
                td.path().join("debian/changelog"),
                r###"blah (0.2) UNRELEASED; urgency=medium

  * debian/changelog: And a change.

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100

blah (0.1) unstable; urgency=medium

  * debian/changelog: Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"###,
            )
            .unwrap();
            let basis_tree = tree.basis_tree().unwrap();
            let lock_read = tree.lock_read();
            let basis_lock_read = basis_tree.lock_read();
            let changes = tree
                .iter_changes(&basis_tree, None, None, None)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(!only_changes_last_changelog_block(
                &tree,
                &tree.basis_tree().unwrap(),
                Path::new("debian/changelog"),
                changes.iter()
            )
            .unwrap());
            std::mem::drop(basis_lock_read);
            std::mem::drop(lock_read);
        }

        #[test]
        fn test_changes_to_last_only() {
            let td = tempfile::tempdir().unwrap();
            let tree = make_package_tree(td.path());
            std::fs::write(
                td.path().join("debian/changelog"),
                r###"blah (0.2) UNRELEASED; urgency=medium

  * And a change.
  * Not a team upload.

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100

blah (0.1) unstable; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"###,
            )
            .unwrap();
            let basis_tree = tree.basis_tree().unwrap();
            let lock_read = tree.lock_read();
            let basis_lock_read = basis_tree.lock_read();
            let changes = tree
                .iter_changes(&basis_tree, None, None, None)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(only_changes_last_changelog_block(
                &tree,
                &tree.basis_tree().unwrap(),
                Path::new("debian/changelog"),
                changes.iter()
            )
            .unwrap());
            std::mem::drop(basis_lock_read);
            std::mem::drop(lock_read);
        }

        #[test]
        fn test_only_new_changelog() {
            use breezyshim::tree::MutableTree;
            let td = tempfile::tempdir().unwrap();
            let tree = create_standalone_workingtree(td.path(), "git").unwrap();
            let lock_write = tree.lock_write();
            std::fs::create_dir_all(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/changelog"),
                r###"blah (0.1) unstable; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"###,
            )
            .unwrap();
            let basis_tree = tree.basis_tree().unwrap();
            let lock_read = tree.lock_read();
            let basis_lock_read = basis_tree.lock_read();
            tree.add(&[Path::new("debian"), Path::new("debian/changelog")])
                .unwrap();
            let changes = tree
                .iter_changes(&basis_tree, None, None, None)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(only_changes_last_changelog_block(
                &tree,
                &tree.basis_tree().unwrap(),
                Path::new("debian/changelog"),
                changes.iter()
            )
            .unwrap());
            std::mem::drop(basis_lock_read);
            std::mem::drop(lock_read);
            std::mem::drop(lock_write);
        }

        #[test]
        fn test_changes_to_last_only_but_released() {
            let td = tempfile::tempdir().unwrap();
            let tree = make_package_tree(td.path());
            std::fs::write(
                td.path().join("debian/changelog"),
                r###"blah (0.2) unstable; urgency=medium

  * And a change.

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100

blah (0.1) unstable; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"###,
            )
            .unwrap();
            tree.build_commit()
                .message("release")
                .committer(COMMITTER)
                .commit()
                .unwrap();
            std::fs::write(
                td.path().join("debian/changelog"),
                r###"blah (0.2) unstable; urgency=medium

  * And a change.
  * Team Upload.

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100

blah (0.1) unstable; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"###,
            )
            .unwrap();
            let basis_tree = tree.basis_tree().unwrap();
            let lock_read = tree.lock_read();
            let basis_lock_read = basis_tree.lock_read();
            let changes = tree
                .iter_changes(&basis_tree, None, None, None)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();

            assert!(!only_changes_last_changelog_block(
                &tree,
                &tree.basis_tree().unwrap(),
                Path::new("debian/changelog"),
                changes.iter()
            )
            .unwrap());
            std::mem::drop(basis_lock_read);
            std::mem::drop(lock_read);
        }
    }
}
