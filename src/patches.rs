use breezyshim::tree::{Tree, WorkingTree, Error as TreeError};
use breezyshim::branch::{Branch};
use breezyshim::RevisionId;
use debian_changelog::ChangeLog;
use std::path::{Path, PathBuf};

// TODO(jelmer): Use debmutate version
pub const DEFAULT_DEBIAN_PATCHES_DIR: &str = "debian/patches";

/// Find the name of the patches directory.
///
/// This will always return a path, even if the patches directory does not yet exist.
///
/// # Arguments
///
/// * `tree` - Tree to check
/// * `subpath` - Subpath to check
///
/// # Returns
///
/// Path to patches directory, or what it should be
pub fn tree_patches_directory(tree: &dyn Tree, subpath: &Path) -> PathBuf {
    find_patches_directory(tree, subpath).unwrap_or(DEFAULT_DEBIAN_PATCHES_DIR.into())
}

/// Find the name of the patches directory in a debian/rules file
pub fn rules_find_patches_directory(mf: &makefile_lossless::Makefile) -> Option<PathBuf> {
    let v = mf.variable_definitions().find(|v| v.name().as_deref() == Some("QUILT_PATCH_DIR"))?;
    v.raw_value().map(PathBuf::from)
}

pub fn find_patches_directory(tree: &dyn Tree, subpath: &Path) -> Option<PathBuf> {
    let rules_path = subpath.join("debian/rules");

    let rules_file = match tree.get_file(&rules_path) {
        Ok(f) => Some(f),
        Err(TreeError::NoSuchFile(_)) => None,
        Err(e) => {
            log::warn!("Failed to read {}: {}", rules_path.display(), e);
            None
        }
    };

    if let Some(rules_file) = rules_file {
        let mf_patch_dir = match makefile_lossless::Makefile::read_relaxed(rules_file) {
            Ok(mf) => {
                rules_find_patches_directory(&mf).or_else(|| {
                    log::debug!("No QUILT_PATCH_DIR in {}", rules_path.display());
                    None
                })
            }
            Err(e) => {
                log::warn!("Failed to parse {}: {}", rules_path.display(), e);
                None
            }
        };

        if let Some(mf_patch_dir) = mf_patch_dir {
            return Some(mf_patch_dir);
        }
    }

    if tree.has_filename(Path::new(DEFAULT_DEBIAN_PATCHES_DIR)) {
        return Some(DEFAULT_DEBIAN_PATCHES_DIR.into());
    }

    None
}

/// Find the base revision to apply patches to.
///
/// * `tree` - Tree to find the patch base for
pub fn find_patch_base(tree: &WorkingTree) -> Option<RevisionId> {
    let f = match tree.get_file(std::path::Path::new("debian/patches/series")) {
        Ok(f) => f,
        Err(TreeError::NoSuchFile(_)) => return None,
        Err(e) => {
            log::warn!("Failed to read debian/patches/series: {}", e);
            return None;
        }
    };
    let cl = match ChangeLog::read(f) {
        Ok(cl) => cl,
        Err(e) => {
            log::warn!("Failed to parse debian/patches/series: {}", e);
            return None;
        }
    };
    let entry = cl.entries().next()?;
    let package = entry.package().unwrap();
    let upstream_version = entry.version().unwrap().upstream_version;
    let possible_tags = vec![
        format!("upstream-{}", upstream_version),
        format!("upstream/{}", upstream_version),
        format!("{}", upstream_version),
        format!("v{}", upstream_version),
        format!("{}-{}", package, upstream_version),
    ];
    let tags = tree.branch().tags().get_tag_dict();
    for possible_tag in possible_tags {
        if let Some(revid) = tags.get(&possible_tag) {
            return Some(revid.clone());
        }
    }
    // TODO(jelmer): Do something clever, like look for the last merge?
    None
}

/// Find the branch that is used to track patches.
///
/// * `tree` - Tree for which to find patches branch
///
/// Returns:
/// A `Branch` instance
pub fn find_patches_branch(tree: &WorkingTree) -> Option<Box<dyn Branch>> {
    let local_branch_name = if let Some(name) = tree.branch().name() {
        name
    } else {
        return None;
    };
    let branch_name = format!("patch-queue/{}", local_branch_name);
    match tree.branch().controldir().open_branch(Some(branch_name.as_str())) {
        Ok(b) => return Some(b),
        Err(NotBranchError) => {},
        Err(e) => {
            log::warn!("Failed to open branch {}: {}", branch_name, e);
        }
    }
    let branch_name = if local_branch_name == "master" {
        "patched".to_string()
    } else {
        format!("patched-{}", local_branch_name)
    };
    match tree.branch().controldir().open_branch(Some(branch_name.as_str())) {
        Ok(b) => return Some(b),
        Err(NotBranchError) => {},
        Err(e) => {
            log::warn!("Failed to open branch {}: {}", branch_name, e);
        }
    }
    None
}
