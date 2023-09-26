use breezyshim::tree::{Error as TreeError, Tree, TreeChange, WorkingTree};
use debian_changelog::ChangeLog;
use debversion::Version;
use lazy_regex::regex_replace;

/// Check whether the only change in a tree is to the last changelog entry.
///
/// # Arguments
/// * `tree`: Tree to analyze
/// * `changelog_path`: Path to the changelog file
/// * `changes`: Changes in the tree
pub fn only_changes_last_changelog_block<'a>(
    tree: &WorkingTree,
    basis_tree: &Box<dyn Tree>,
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
        Err(TreeError::NoSuchFile(_)) => {
            return Ok(false);
        }
        Err(TreeError::Other(e)) => {
            panic!("Error reading changelog: {}", e);
        }
    };
    let mut old_cl = match basis_tree.get_file(changelog_path) {
        Ok(f) => ChangeLog::read(f)?,
        Err(TreeError::NoSuchFile(_)) => {
            return Ok(true);
        }
        Err(TreeError::Other(e)) => {
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

/// Increment a version number.
///
/// For native packages, increment the main version number.
/// For other packages, increment the debian revision.
///
/// # Arguments
///
///  * `v`: Version to increment (modified in place)
pub fn increment_version(v: &mut Version) {
    if v.debian_revision.is_some() {
        v.debian_revision = v.debian_revision.as_ref().map(|v| {
            {
                regex_replace!(r"\d+$", v, |x: &str| (x.parse::<i32>().unwrap() + 1)
                    .to_string())
            }
            .to_string()
        });
    } else {
        v.upstream_version = regex_replace!(r"\d+$", v.upstream_version.as_ref(), |x: &str| (x
            .parse::<i32>()
            .unwrap()
            + 1)
        .to_string())
        .to_string();
    }
}
