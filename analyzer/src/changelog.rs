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

pub fn find_last_distribution(cl: &ChangeLog) -> Option<String> {
    for block in cl.entries() {
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

pub const DEBIAN_POCKETS: &[&str] = &["", "-security", "-proposed-updates", "-backports"];
pub const UBUNTU_POCKETS: &[&str] = &["", "-proposed", "-updates", "-security", "-backports"];

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
        .flat_map(|r| DEBIAN_POCKETS.iter().map(move |t| format!("{}{}", r, t)))
        .collect::<Vec<_>>();
    let all_ubuntu = crate::release_info::ubuntu_releases()
        .iter()
        .flat_map(|r| UBUNTU_POCKETS.iter().map(move |t| format!("{}{}", r, t)))
        .collect::<Vec<_>>();
    let match_targets = if all_debian.contains(&current_target) {
        vec![current_target]
    } else if all_ubuntu.contains(&current_target) {
        let mut match_targets = crate::release_info::ubuntu_releases();
        if current_target.contains('-') {
            let distro = current_target.split('-').next().unwrap();
            match_targets.extend(DEBIAN_POCKETS.iter().map(|r| format!("{}{}", r, distro)));
        }
        match_targets
    } else {
        // If we do not recognize the current target in order to apply special
        // rules to it, then just assume that only previous uploads to exactly
        // the same target count.
        vec![current_target]
    };
    for block in changelog.entries().skip(1) {
        if match_targets.contains(&block.distributions().unwrap()[0]) {
            return block.version().clone();
        }
    }

    None
}

#[cfg(test)]
mod tests {
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
}
