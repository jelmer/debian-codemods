use breezyshim::branch::Branch;
use breezyshim::delta::filter_excluded;
use breezyshim::error::Error as BrzError;
use breezyshim::patches::AppliedPatches;
use breezyshim::tree::{MutableTree, Tree, WorkingTree};
use breezyshim::transform::{TreeTransform, PreviewTree};
use breezyshim::workspace::reset_tree;
use breezyshim::RevisionId;
use debian_changelog::ChangeLog;
use patchkit::patch::UnifiedPatch;
use std::io::Write;
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

#[cfg(test)]
mod tree_patches_directory_tests {
    use breezyshim::tree::MutableTree;
    #[test]
    fn test_simple() {
        let td = tempfile::tempdir().unwrap();
        let local_tree = breezyshim::controldir::create_standalone_workingtree(
            td.path(),
            &breezyshim::controldir::ControlDirFormat::default(),
        )
        .unwrap();
        assert_eq!(
            super::tree_patches_directory(&local_tree, std::path::Path::new("")),
            std::path::Path::new("debian/patches")
        );
    }

    #[test]
    fn test_default() {
        let td = tempfile::tempdir().unwrap();
        let local_tree = breezyshim::controldir::create_standalone_workingtree(
            td.path(),
            &breezyshim::controldir::ControlDirFormat::default(),
        )
        .unwrap();
        local_tree.mkdir(std::path::Path::new("debian")).unwrap();
        local_tree
            .mkdir(std::path::Path::new("debian/patches"))
            .unwrap();
        assert_eq!(
            super::tree_patches_directory(&local_tree, std::path::Path::new("")),
            std::path::Path::new("debian/patches")
        );
    }

    #[test]
    fn test_custom() {
        let td = tempfile::tempdir().unwrap();
        let local_tree = breezyshim::controldir::create_standalone_workingtree(
            td.path(),
            &breezyshim::controldir::ControlDirFormat::default(),
        )
        .unwrap();
        local_tree.mkdir(std::path::Path::new("debian")).unwrap();
        local_tree
            .mkdir(std::path::Path::new("debian/patches"))
            .unwrap();
        local_tree
            .put_file_bytes_non_atomic(
                std::path::Path::new("debian/rules"),
                br#"
QUILT_PATCH_DIR := debian/patches-applied

all:

blah: bloe
	foo

"#,
            )
            .unwrap();
        assert_eq!(
            super::tree_patches_directory(&local_tree, std::path::Path::new("")),
            std::path::Path::new("debian/patches-applied")
        );
    }
}

/// Find the name of the patches directory in a debian/rules file
pub fn rules_find_patches_directory(mf: &makefile_lossless::Makefile) -> Option<PathBuf> {
    let v = mf
        .variable_definitions()
        .find(|v| v.name().as_deref() == Some("QUILT_PATCH_DIR"))?;
    v.raw_value().map(PathBuf::from)
}

#[test]
fn test_rules_find_patches_directory() {
    let mf = makefile_lossless::Makefile::read_relaxed(
        &br#"QUILT_PATCH_DIR := debian/patches-applied
"#[..],
    )
    .unwrap();
    assert_eq!(
        rules_find_patches_directory(&mf),
        Some(PathBuf::from("debian/patches-applied"))
    );
}

pub fn find_patches_directory(tree: &dyn Tree, subpath: &Path) -> Option<PathBuf> {
    let rules_path = subpath.join("debian/rules");

    let rules_file = match tree.get_file(&rules_path) {
        Ok(f) => Some(f),
        Err(BrzError::NoSuchFile(_)) => None,
        Err(e) => {
            log::warn!("Failed to read {}: {}", rules_path.display(), e);
            None
        }
    };

    if let Some(rules_file) = rules_file {
        let mf_patch_dir = match makefile_lossless::Makefile::read_relaxed(rules_file) {
            Ok(mf) => rules_find_patches_directory(&mf).or_else(|| {
                log::debug!("No QUILT_PATCH_DIR in {}", rules_path.display());
                None
            }),
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
        Err(BrzError::NoSuchFile(_)) => return None,
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
    let tags = tree.branch().tags().unwrap().get_tag_dict().unwrap();
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
    let local_branch_name = tree.branch().name()?;
    let branch_name = format!("patch-queue/{}", local_branch_name);
    match tree
        .branch()
        .controldir()
        .open_branch(Some(branch_name.as_str()))
    {
        Ok(b) => return Some(b),
        Err(BrzError::NotBranchError(..)) => {}
        Err(e) => {
            log::warn!("Failed to open branch {}: {}", branch_name, e);
        }
    }
    let branch_name = if local_branch_name == "master" {
        "patched".to_string()
    } else {
        format!("patched-{}", local_branch_name)
    };
    match tree
        .branch()
        .controldir()
        .open_branch(Some(branch_name.as_str()))
    {
        Ok(b) => return Some(b),
        Err(BrzError::NotBranchError(..)) => {}
        Err(e) => {
            log::warn!("Failed to open branch {}: {}", branch_name, e);
        }
    }
    None
}

/// Add a new patch.
///
/// # Arguments
/// * `tree` - Tree to edit
/// * `patches_directory` - Name of patches directory
/// * `name` - Patch name without suffix
/// * `contents` - Diff
/// * `header` - RFC822 to read
///
/// Returns:
/// Name of the patch that was written (including suffix)
pub fn add_patch(
    tree: &WorkingTree,
    patches_directory: &Path,
    name: &str,
    contents: &[u8],
    header: Option<dep3::PatchHeader>,
) -> Result<(Vec<std::path::PathBuf>, String), String> {
    if !tree.has_filename(patches_directory) {
        let parent = patches_directory.parent().unwrap();
        if !tree.has_filename(parent) {
            tree.mkdir(parent)
                .expect("Failed to create parent directory");
        }
        tree.mkdir(patches_directory).unwrap();
    }
    let series_path = patches_directory.join("series");
    let mut series = match tree.get_file(&series_path) {
        Ok(f) => patchkit::quilt::Series::read(f).unwrap(),
        Err(BrzError::NoSuchFile(_)) => patchkit::quilt::Series::new(),
        Err(e) => {
            return Err(format!("Failed to read {}: {}", series_path.display(), e));
        }
    };

    let patch_suffix =
        patchkit::quilt::find_common_patch_suffix(series.patches()).unwrap_or(".patch");
    let patchname = format!("{}{}", name, patch_suffix);
    let path = patches_directory.join(patchname.as_str());
    if tree.has_filename(path.as_path()) {
        return Err(format!("Patch {} already exists", patchname));
    }

    let mut patch_contents = Vec::new();
    if let Some(header) = header {
        header.write(&mut patch_contents).unwrap();
    }
    patch_contents.write_all(b"---\n").unwrap();
    patch_contents.write_all(contents).unwrap();
    tree.put_file_bytes_non_atomic(&path, patch_contents.as_slice())
        .map_err(|e| format!("Failed to write patch: {}", e))?;

    // TODO(jelmer): Write to patches branch if applicable

    series.append(patchname.as_str(), None);
    let mut series_bytes = Vec::new();
    series
        .write(&mut series_bytes)
        .map_err(|e| format!("Failed to write series: {}", e))?;
    tree.put_file_bytes_non_atomic(&series_path, series_bytes.as_slice())
        .map_err(|e| format!("Failed to write series: {}", e))?;
    tree.add(&[series_path.as_path(), path.as_path()])
        .map_err(|e| format!("Failed to add patch: {}", e))?;

    let specific_files = vec![series_path, path];

    Ok((specific_files, patchname))
}

/// Move upstream changes to patch.
///
/// # Arguments
///
/// * `local_tree` - Local tree
/// * `basis_tree` - Basis tree
/// * `subpath` - Subpath
/// * `patch_name` - Suggested patch name
/// * `description` - Description
pub fn move_upstream_changes_to_patch(
    local_tree: &WorkingTree,
    basis_tree: &dyn Tree,
    subpath: &std::path::Path,
    patch_name: &str,
    description: &str,
    dirty_tracker: Option<&breezyshim::dirty_tracker::DirtyTracker>,
    timestamp: Option<chrono::NaiveDate>,
) -> Result<(Vec<std::path::PathBuf>, String), String> {
    let timestamp = if let Some(timestamp) = timestamp {
        timestamp
    } else {
        chrono::Utc::now().naive_utc().date()
    };
    let mut diff = Vec::new();
    breezyshim::diff::show_diff_trees(basis_tree, local_tree, &mut diff, None, None)
        .map_err(|e| format!("Failed to generate diff: {}", e))?;
    reset_tree(local_tree, Some(basis_tree), Some(subpath), dirty_tracker)
        .map_err(|e| format!("Failed to reset tree: {}", e))?;
    // See https://dep-team.pages.debian.net/deps/dep3/ for fields.
    let mut dep3_header = dep3::PatchHeader::new();
    dep3_header.set_description(description);
    dep3_header.set_origin(None, dep3::Origin::Other("other".to_string()));
    dep3_header.set_last_update(timestamp);
    let patches_directory = subpath.join(tree_patches_directory(local_tree, subpath));
    let (specific_files, patchname) = add_patch(
        local_tree,
        &patches_directory,
        patch_name,
        diff.as_slice(),
        Some(dep3_header),
    )?;
    Ok((specific_files, patchname))
}

#[cfg(test)]
mod move_upstream_changes_to_patch_tests {
    use breezyshim::controldir::ControlDirFormat;
    use breezyshim::tree::MutableTree;
    #[test]
    fn test_simple() {
        breezyshim::init();
        let td = tempfile::tempdir().unwrap();
        let local_tree = breezyshim::controldir::create_standalone_workingtree(
            td.path(),
            &ControlDirFormat::default(),
        )
        .unwrap();

        std::fs::write(td.path().join("foo"), b"foo\n").unwrap();
        local_tree.mkdir(std::path::Path::new("debian")).unwrap();
        local_tree.add(&[std::path::Path::new("foo")]).unwrap();

        super::move_upstream_changes_to_patch(
            &local_tree,
            &local_tree.basis_tree(),
            std::path::Path::new(""),
            "patch",
            "This is a description",
            None,
            Some(chrono::NaiveDate::from_ymd(2020, 1, 1)),
        )
        .unwrap();

        let path = td.path();

        assert!(!path.join("foo").exists());
        assert!(path.join("debian/patches").exists());
        assert!(path.join("debian/patches/series").exists());
        assert!(path.join("debian/patches/patch.patch").exists());

        let series = std::fs::read_to_string(path.join("debian/patches/series")).unwrap();
        assert_eq!(series, "patch.patch\n");

        let patch = std::fs::read_to_string(path.join("debian/patches/patch.patch")).unwrap();
        assert!(
            patch.starts_with(
                r#"Description: This is a description
Origin: other
Last-Update: 2020-01-01
---
"#
            ),
            "{:?}",
            patch
        );

        assert!(
            patch.ends_with(
                r#"@@ -0,0 +1,1 @@
+foo

"#
            ),
            "{:?}",
            patch
        );
    }
}

pub fn read_quilt_patches<'a>(tree: &'a dyn Tree, directory: &'a std::path::Path) -> impl Iterator<Item = UnifiedPatch> + 'a {
    let series_path = directory.join("series");
    let series = match tree.get_file(series_path.as_path()) {
        Ok(series) => patchkit::quilt::Series::read(series).unwrap(),
        Err(BrzError::NoSuchFile(..)) => { patchkit::quilt::Series::new() },
        Err(e) => panic!("error reading series: {:?}", e)
    };

    let mut ret = vec![];
    for patch in series.patches() {
        let p = directory.join(patch);
        let lines = tree.get_file_lines(p.as_path()).unwrap();
        // TODO(jelmer): Pass on options?
        ret.push(patchkit::patch::UnifiedPatch::parse_patches(lines.into_iter()).unwrap());
    }
    ret.into_iter().flatten()
}

pub fn upstream_with_applied_patches(tree: &WorkingTree, patches: Vec<UnifiedPatch>) -> Result<Box<dyn Tree>, BrzError> {
    if let Some(patches_branch) = find_patches_branch(tree) {
        // TODO(jelmer): Make sure it's actually rebased on current upstream
        patches_branch.basis_tree().map(|x| Box::from(x) as Box<dyn Tree>)
    } else {
        let upstream_revision = find_patch_base(tree).unwrap(); // TODO: raise PatchApplicationBaseNotFound
        let upstream_tree = tree.branch().repository().revision_tree(&upstream_revision)?;
        Ok(Box::new(AppliedPatches::new(&upstream_tree, patches, None)?) as Box<dyn Tree>)
    }
}

pub fn tree_non_patches_changes(tree: &WorkingTree, patches_directory: Option<&std::path::Path>) -> impl Iterator<Item = breezyshim::tree::TreeChange> {
    let patches = match patches_directory {
        Some(directory) => read_quilt_patches(tree, directory).collect::<Vec<_>>(),
        None => vec![]
    };

    // TODO(jelmer): what if the patches are already applied in the tree?

    let upstream_patches_tree = upstream_with_applied_patches(tree, patches.clone()).unwrap();
    let patches_tree = AppliedPatches::new(tree, patches, None).unwrap();
    let changes = patches_tree.iter_changes(upstream_patches_tree.as_ref(), None, None, None).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    let  paths = [std::path::Path::new("debian")];
    filter_excluded(changes.into_iter(), &paths[..]).filter(|change| {
        change.path.1.as_deref() != Some(std::path::Path::new(""))
    }).collect::<Vec<_>>().into_iter()
}
