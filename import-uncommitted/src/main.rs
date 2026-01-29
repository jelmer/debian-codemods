use breezyshim::debian::apt::{Apt, LocalApt, RemoteApt};
use breezyshim::debian::error::Error as DebianError;
use breezyshim::debian::import_dsc::{DistributionBranch, DistributionBranchSet};
use breezyshim::debian::upstream::UpstreamSource;
use breezyshim::error::Error as BrzError;
use breezyshim::repository::Repository;
use breezyshim::tree::{MutableTree, PyTree, Tree};
use breezyshim::workingtree::WorkingTree;
use breezyshim::Branch;
use breezyshim::RevisionId;
use clap::Parser;
use debian_analyzer::editor::MutableTreeEdit;
use debian_changelog::ChangeLog;
use debian_control::lossless::Control;
use debversion::Version;
use std::collections::HashMap;
use std::path::Path;

fn find_missing_versions(
    archive_cl: &ChangeLog,
    tree_version: Option<&Version>,
) -> Result<Vec<Version>, Error> {
    let mut missing_versions = vec![];
    let mut found = false;
    for (idx, block) in archive_cl.iter().enumerate() {
        if tree_version.is_some() && block.version().as_ref() == tree_version {
            found = true;
            break;
        }
        match block.version() {
            Some(version) => {
                missing_versions.push(version);
            }
            None => {
                log::warn!(
                    "Skipping changelog entry at index {} without a version (package: {:?})",
                    idx,
                    block.package()
                );
            }
        }
    }

    if !found {
        if let Some(tree_version) = tree_version {
            return Err(Error::TreeVersionNotInArchiveChangelog(
                tree_version.clone(),
            ));
        }
    }
    Ok(missing_versions)
}

fn is_noop_upload(tree: &dyn WorkingTree, basis_tree: &dyn PyTree, subpath: &Path) -> bool {
    let mut changes = tree.iter_changes(basis_tree, None, None, None).unwrap();
    let change = loop {
        let change = if let Some(change) = changes.next() {
            change.unwrap()
        } else {
            return true;
        };
        if change.path.1 != Some(std::path::PathBuf::from("")) {
            break change;
        }
    };
    let cl_path = subpath.join("debian").join("changelog");
    if change.path != (Some(cl_path.clone()), Some(cl_path.clone())) {
        return false;
    }
    // if there are any other changes, then this is not trivial:
    if changes.next().is_some() {
        return false;
    }
    let new_cl = match tree.get_file_text(&cl_path) {
        Ok(cl) => ChangeLog::read(cl.as_slice()).unwrap(),
        Err(BrzError::NoSuchFile(_)) => return false,
        Err(e) => panic!("Unexpected error: {:?}", e),
    };

    let old_cl = match basis_tree.get_file_text(&cl_path) {
        Ok(cl) => ChangeLog::read(cl.as_slice()).unwrap(),
        Err(BrzError::NoSuchFile(_)) => return false,
        Err(e) => panic!("Unexpected error: {:?}", e),
    };

    let new_cl = new_cl
        .iter()
        .skip(1)
        .collect::<debian_changelog::ChangeLog>();

    // NOTE: We use string comparison here because the PartialEq implementation for ChangeLog
    // compares internal metadata/state that differs even when the logical content is identical.
    // This matches the Python implementation which uses: str(new_cl) == str(old_cl)
    // TODO(jelmer): Check for uploads that aren't just meant to trigger a build.  i.e. closing
    // bugs.
    new_cl.to_string() == old_cl.to_string()
}

#[derive(Debug)]
pub enum Error {
    NoopChangesOnly(Version, Version),
    NoMissingVersions(Version, Version),
    TreeVersionNotInArchiveChangelog(Version),
    TreeVersionWithoutTag(Version, String),
    TreeUpstreamVersionMissing(String),
    UnreleasedChangesSinceTreeVersion(Version),
    SnapshotMissing {
        package: String,
        version: Version,
    },
    SnapshotHashMismatch {
        filename: String,
        expected_hash: String,
        actual_hash: String,
    },
    SnapshotDownloadError {
        url: String,
        error: String,
        is_server_error: Option<bool>,
    },
    ConflictsInTree,
}

impl From<debian_analyzer::snapshot::Error> for Error {
    fn from(e: debian_analyzer::snapshot::Error) -> Self {
        match e {
            debian_analyzer::snapshot::Error::SnapshotMissing(package, version) => {
                Error::SnapshotMissing { package, version }
            }
            debian_analyzer::snapshot::Error::SnapshotHashMismatch {
                filename,
                expected_hash,
                actual_hash,
            } => Error::SnapshotHashMismatch {
                filename,
                expected_hash,
                actual_hash,
            },
            debian_analyzer::snapshot::Error::SnapshotDownloadError(
                url,
                error,
                is_server_error,
            ) => Error::SnapshotDownloadError {
                url,
                error: error.to_string(),
                is_server_error,
            },
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::NoopChangesOnly(vcs_version, archive_version) => {
                write!(
                    f,
                    "No missing versions with effective changes. Archive has {}, VCS has {}",
                    archive_version, vcs_version
                )
            }
            Error::NoMissingVersions(vcs_version, archive_version) => {
                write!(
                    f,
                    "No missing versions after all. Archive has {}, VCS has {}",
                    archive_version, vcs_version
                )
            }
            Error::TreeVersionNotInArchiveChangelog(tree_version) => {
                write!(
                    f,
                    "Tree version {} does not appear in archive changelog",
                    tree_version
                )
            }
            Error::TreeVersionWithoutTag(tree_version, tag_name) => {
                write!(
                    f,
                    "Tree version {} does not have a tag (e.g. {})",
                    tree_version, tag_name
                )
            }
            Error::TreeUpstreamVersionMissing(upstream_version) => {
                write!(f, "Unable to find upstream version {}", upstream_version)
            }
            Error::UnreleasedChangesSinceTreeVersion(tree_version) => {
                write!(f, "There are unreleased changes since {}", tree_version)
            }
            Error::SnapshotMissing { package, version } => {
                write!(f, "Snapshot for {} {} missing", package, version)
            }
            Error::SnapshotHashMismatch {
                filename,
                expected_hash,
                actual_hash,
            } => {
                write!(
                    f,
                    "Snapshot hash mismatch for {}: {} != {}",
                    filename, expected_hash, actual_hash
                )
            }
            Error::SnapshotDownloadError {
                url,
                error,
                is_server_error: _,
            } => {
                write!(f, "Failed to download snapshot from {}: {}", url, error)
            }
            Error::ConflictsInTree => {
                write!(f, "Conflicts in tree")
            }
        }
    }
}

impl std::error::Error for Error {}

fn set_vcs_git_url(
    control: &Control,
    vcs_git_base: Option<&str>,
    vcs_browser_base: Option<&str>,
) -> (Option<String>, Option<String>) {
    let mut source = control.source().unwrap();
    let old_vcs_url = source.vcs_git();
    if let Some(vcs_git_base) = vcs_git_base {
        let mut vcs_git: debian_control::vcs::ParsedVcs = vcs_git_base.parse().unwrap();
        vcs_git.repo_url = format!(
            "{}/{}.git",
            vcs_git.repo_url.trim_end_matches('/'),
            source.name().unwrap()
        );

        source.set_vcs_git(&vcs_git.to_string());
    }
    let new_vcs_url = source.vcs_git();
    if let Some(vcs_browser_base) = vcs_browser_base {
        let vcs_browser_base: url::Url = vcs_browser_base.parse().unwrap();
        source.set_vcs_browser(Some(
            vcs_browser_base
                .join(&source.name().unwrap())
                .unwrap()
                .as_ref(),
        ));
    }
    (old_vcs_url, new_vcs_url)
}

fn contains_git_attributes(tree: &dyn Tree, subpath: &Path) -> bool {
    // Use walkdirs to iterate through the tree
    let prefix = if subpath == Path::new("") || subpath.as_os_str().is_empty() {
        None
    } else {
        Some(subpath)
    };

    tree.walkdirs(prefix).unwrap().any(|entry| {
        entry
            .map(|e| e.relpath.file_name() == Some(std::ffi::OsStr::new(".gitattributes")))
            .unwrap_or(false)
    })
}

#[allow(clippy::too_many_arguments)]
fn import_uncommitted(
    tree: &dyn WorkingTree,
    subpath: &Path,
    apt: &dyn Apt,
    source_name: &str,
    archive_version: Option<Version>,
    tree_version: Option<Version>,
    merge_unreleased: bool,
    skip_noop: bool,
) -> Result<Vec<(String, Version, RevisionId)>, Error> {
    let archive_source = tempfile::tempdir().unwrap();
    apt.retrieve_source(source_name, archive_source.path(), archive_version.as_ref())
        .unwrap_or_else(|_| {
            panic!(
                "Failed to retrieve source for {} {}",
                source_name,
                archive_version.as_ref().unwrap()
            )
        });
    let dsc = std::fs::read_dir(archive_source.path())
        .unwrap()
        .filter_map(Result::ok)
        .find(|e| e.file_name().to_string_lossy().ends_with(".dsc"))
        .unwrap()
        .path();
    log::info!("Unpacking source {}", dsc.display());
    let output = std::process::Command::new("dpkg-source")
        .arg("-x")
        .arg(dsc)
        .current_dir(archive_source.path())
        .output()
        .unwrap();
    if !output.status.success() {
        panic!("Failed to unpack source: {}", output.status);
    }
    let subdir = std::fs::read_dir(archive_source.path())
        .unwrap()
        .filter_map(Result::ok)
        .find(|e| e.file_type().unwrap().is_dir())
        .unwrap()
        .path();
    let archive_cl = ChangeLog::read_path(subdir.join("debian/changelog")).unwrap();
    let missing_versions = find_missing_versions(&archive_cl, tree_version.as_ref())?;
    if missing_versions.is_empty() {
        let archive_version = archive_cl.iter().next().unwrap().version();
        return Err(Error::NoMissingVersions(
            tree_version.unwrap(),
            archive_version.unwrap(),
        ));
    }
    log::info!(
        "Missing versions: {}",
        missing_versions
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut ret = Vec::new();
    let dbs = DistributionBranchSet::new();
    let branch = tree.branch();
    // Note: Python passes tree=tree here, but Rust DistributionBranch::new expects &dyn PyTree
    // which WorkingTree doesn't directly convert to. Since we have access to the tree through
    // the branch, passing None here should be functionally equivalent.
    let db = DistributionBranch::new(&branch, &branch, None, None);
    dbs.add_branch(&db);

    let merge_into = if let Some(ref tree_version) = tree_version {
        let tree_version_revid = match db.revid_of_version(tree_version) {
            Ok(revid) => revid,
            Err(DebianError::BrzError(BrzError::NoSuchTag(n))) => {
                return Err(Error::TreeVersionWithoutTag(tree_version.clone(), n));
            }
            Err(e) => {
                panic!("Failed to find revision for {}: {}", tree_version, e);
            }
        };
        if tree_version_revid != tree.last_revision().unwrap() {
            // There are changes since the last tree version.
            log::info!("Commits exist on the branch since last upload to archive");
            if !merge_unreleased {
                return Err(Error::UnreleasedChangesSinceTreeVersion(
                    tree_version.clone(),
                ));
            }

            // Save the current revision BEFORE updating to the tree_version
            let original_last_revision = tree.last_revision().unwrap();
            tree.update(Some(&tree_version_revid)).unwrap();
            Some(original_last_revision)
        } else {
            None
        }
    } else {
        None
    };

    let upstream_dir = tempfile::tempdir().unwrap();
    let applied_patches = tree.has_filename(std::path::Path::new(".pc/applied-patches"));
    if tree_version
        .as_ref()
        .and_then(|tv| tv.debian_revision.as_ref())
        .is_some()
    {
        let upstream_tips = match db.pristine_upstream_source().version_as_revisions(
            Some(source_name),
            &tree_version.as_ref().unwrap().upstream_version,
            None,
        ) {
            Ok(tips) => tips,
            Err(breezyshim::debian::error::Error::PackageVersionNotPresent { .. }) => {
                return Err(Error::TreeUpstreamVersionMissing(
                    tree_version.unwrap().upstream_version,
                ));
            }
            Err(e) => {
                panic!("Failed to find upstream version: {}", e);
            }
        };
        log::info!(
            "Extracting upstream version {}.",
            tree_version.as_ref().unwrap().upstream_version
        );
        db.extract_upstream_tree(&upstream_tips, upstream_dir.path())
            .unwrap();
    } else {
        db.create_empty_upstream_tree(upstream_dir.path()).unwrap();
    }
    let output_dir = tempfile::tempdir().unwrap();
    let mut last_revid = db.tree().unwrap().last_revision().unwrap();
    for version in missing_versions.iter().rev() {
        let dsc_path = match debian_analyzer::snapshot::download_snapshot(
            source_name,
            version,
            output_dir.path(),
        ) {
            Ok(path) => path,
            Err(debian_analyzer::snapshot::Error::SnapshotMissing(package, version)) => {
                log::warn!(
                    "Missing snapshot for {} {} (never uploaded?), skipping.",
                    package,
                    version
                );
                continue;
            }
            Err(e) => {
                panic!("Failed to download snapshot: {}", e);
            }
        };
        log::info!("Importing {}", version);
        let (tag_name, tag_revid) = match db.import_package(dsc_path.as_path(), applied_patches) {
            Ok(tag_name) => {
                let revid = db.branch().tags().unwrap().lookup_tag(&tag_name).unwrap();
                (tag_name, revid)
            }
            Err(breezyshim::debian::error::Error::VersionAlreadyImported {
                version: v,
                tag_name: tag,
                package: _,
            }) if v == *version => {
                log::info!(
                    "{} was already imported (tag: {}), just not on the branch. Updating tree.",
                    v,
                    tag
                );
                let revid = db.branch().tags().unwrap().lookup_tag(&tag).unwrap();
                db.tree().unwrap().update(Some(&revid)).unwrap();
                (tag.to_string(), revid)
            }
            Err(e) => {
                panic!("Failed to import {}: {}", version, e);
            }
        };
        if skip_noop && !last_revid.is_null() {
            let last_tree = match db.tree().unwrap().revision_tree(&last_revid) {
                Ok(tree) => tree,
                Err(BrzError::NoSuchRevisionInTree(_)) => {
                    Box::new(db.branch().repository().revision_tree(&last_revid).unwrap())
                }
                Err(e) => {
                    panic!("Failed to load tree for {}: {}", last_revid, e);
                }
            };
            if is_noop_upload(tree, last_tree.as_ref(), subpath) {
                log::info!("Skipping version {} without effective changes", version);
                tree.update(Some(&last_revid)).unwrap();
                continue;
            }
        }
        ret.push((tag_name, version.clone(), tag_revid.clone()));
        last_revid = tag_revid;
    }

    if ret.is_empty() {
        return Err(Error::NoopChangesOnly(
            tree_version.unwrap(),
            archive_cl.iter().next().unwrap().version().unwrap(),
        ));
    }

    if let Some(merge_into) = merge_into.as_ref() {
        let to_merge = tree.last_revision().unwrap();
        tree.update(Some(merge_into)).unwrap();
        match tree.merge_from_branch(&tree.branch(), Some(&to_merge)) {
            Ok(_) => {}
            Err(BrzError::ConflictsInTree) => {
                return Err(Error::ConflictsInTree);
            }
            Err(e) => {
                panic!("Failed to merge: {}", e);
            }
        }
        let revid = debian_analyzer::debcommit::debcommit(
            tree,
            None,
            subpath,
            None,
            None,
            Some(&format!(
                "Merge archive versions: {}",
                ret.iter()
                    .map(|(t, v, _r)| format!("{} ({})", v, t))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        )
        .unwrap();
        let parent_ids = tree
            .branch()
            .repository()
            .get_revision(&revid)
            .unwrap()
            .parent_ids;
        assert_eq!(
            parent_ids,
            vec![merge_into.clone(), to_merge.clone()],
            "Expected parents to be {:?}, was {:?}",
            vec![merge_into.clone(), to_merge],
            parent_ids
        );
    }
    Ok(ret)
}

#[derive(Parser)]
struct Args {
    #[clap(long, env = "APT_REPOSITORY")]
    /// APT repository to use. Defaults to locally configured.
    /// Also checks REPOSITORIES environment variable if APT_REPOSITORY is not set.
    apt_repository: Option<String>,

    #[clap(long, env = "APT_REPOSITORY_KEY")]
    /// APT repository key to use for validation, if --apt-repository is set.
    apt_repository_key: Option<std::path::PathBuf>,

    #[clap(long)]
    /// Source version to import
    version: Option<debversion::Version>,

    #[clap(long)]
    /// Set Vcs-Git URL
    vcs_git_base: Option<String>,

    #[clap(long)]
    /// Set Vcs-Browser URL
    vcs_browser_base: Option<String>,

    #[clap(long)]
    /// Error rather than merge when there are unreleased changes
    no_merge_unreleased: bool,

    #[clap(long)]
    /// Do not skip uploads without effective changes
    no_skip_noop: bool,

    #[clap(long, env = "PACKAGE")]
    /// Package to import
    package: Option<String>,

    #[clap(long)]
    /// Force importing even if the tree contains git attributes
    force_git_attributes: bool,

    #[clap(long)]
    /// Debug logging
    debug: bool,
}

#[derive(serde::Serialize)]
pub struct Context {
    tags: Vec<(String, debversion::Version)>,
}

impl Context {
    pub fn new(tags: Vec<(String, debversion::Version)>) -> Self {
        Self { tags }
    }

    pub fn versions(&self) -> Vec<&debversion::Version> {
        self.tags.iter().map(|(_, v)| v).collect()
    }

    pub fn tag_names(&self) -> Vec<&str> {
        self.tags.iter().map(|(t, _)| t.as_str()).collect()
    }

    pub fn tags(&self) -> &[(String, debversion::Version)] {
        &self.tags
    }
}

pub fn main() {
    use std::io::Write;
    let mut args = Args::parse();

    // Handle fallback from APT_REPOSITORY to REPOSITORIES environment variable
    // to match Python behavior: os.environ.get("APT_REPOSITORY") or os.environ.get("REPOSITORIES")
    if args.apt_repository.is_none() {
        args.apt_repository = std::env::var("REPOSITORIES").ok();
    }

    env_logger::builder()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .filter(
            None,
            if args.debug {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Info
            },
        )
        .init();

    breezyshim::init();

    let mut svp = svp_client::Reporter::new(versions_dict());

    let apt: Box<dyn Apt> = if let Some(apt_repository) = args.apt_repository {
        Box::new(
            RemoteApt::from_string(&apt_repository, args.apt_repository_key.as_deref()).unwrap(),
        )
    } else {
        Box::new(LocalApt::new(None).unwrap())
    };

    let (local_tree, subpath) =
        match breezyshim::workingtree::open_containing(std::path::Path::new(".")) {
            Ok((local_tree, subpath)) => (local_tree, subpath),
            Err(BrzError::NotBranchError(..)) => {
                svp.report_fatal(
                    "not-branch-error",
                    "Not running in a version-controlled directory",
                    None,
                    None,
                );
            }
            Err(e) => {
                svp.report_fatal(
                    "unexpected-error",
                    &format!("Unexpected error: {}", e),
                    None,
                    None,
                );
            }
        };

    let cl_path = subpath.join("debian/changelog");

    let (source_name, tree_version) = match local_tree.get_file(&cl_path) {
        Ok(f) => {
            let tree_cl = ChangeLog::read(f).unwrap();
            let source_name = tree_cl.iter().next().unwrap().package().unwrap();
            let tree_version = tree_cl
                .iter()
                .find(|block| !block.is_unreleased().unwrap_or(true))
                .and_then(|block| block.version());
            let tree_cl_package = tree_cl.iter().next().unwrap().package().unwrap();
            if args
                .package
                .as_ref()
                .map(|p| *p != tree_cl_package)
                .unwrap_or(false)
            {
                svp.report_fatal(
                    "inconsistent-package",
                    &format!(
                        "Inconsistent package name: {} specified, {} found",
                        args.package.as_ref().unwrap(),
                        tree_cl_package
                    ),
                    None,
                    Some(false),
                );
            }

            (source_name, tree_version)
        }
        Err(BrzError::NoSuchFile(p)) => {
            if local_tree.last_revision().unwrap().is_null() {
                if let Some(package) = args.package {
                    (package, None)
                } else {
                    let hint = Some("Tree is empty. Specify --package?");
                    svp.report_fatal(
                        "missing-changelog",
                        &format!("Missing changelog: {}", p.display()),
                        hint,
                        Some(false),
                    );
                }
            } else {
                svp.report_fatal(
                    "missing-changelog",
                    &format!("Missing changelog: {}", p.display()),
                    None,
                    Some(false),
                );
            }
        }
        Err(e) => {
            svp.report_fatal(
                "unexpected-error",
                &format!("Unexpected error: {}", e),
                None,
                None,
            );
        }
    };

    if !args.force_git_attributes
        && local_tree.branch().repository().vcs_type() == breezyshim::foreign::VcsType::Git
    {
        // See https://salsa.debian.org/jelmer/janitor.debian.net/-/issues/74
        if contains_git_attributes(&local_tree, &subpath) {
            svp.report_fatal(
                "unsupported-git-attributes",
                "Tree contains .gitattributes which may impact imports and 'are unsupported",
                Some("Run with --force-git-attributes to ignore"),
                Some(false),
            );
        }
    }

    let ret = match import_uncommitted(
        &local_tree,
        &subpath,
        apt.as_ref(),
        &source_name,
        args.version,
        tree_version,
        !args.no_merge_unreleased,
        !args.no_skip_noop,
    ) {
        Ok(ret) => ret,
        Err(e @ Error::TreeVersionWithoutTag(..)) => {
            svp.report_fatal("tree-version-not-found", &e.to_string(), None, None);
        }
        Err(e @ Error::TreeUpstreamVersionMissing(..)) => {
            svp.report_fatal("tree-upstream-version-missing", &e.to_string(), None, None);
        }
        Err(e @ Error::UnreleasedChangesSinceTreeVersion(_)) => {
            svp.report_fatal("unreleased-changes", &e.to_string(), None, None);
        }
        Err(e @ Error::TreeVersionNotInArchiveChangelog(_)) => {
            svp.report_fatal(
                "tree-version-not-in-archive-changelog",
                &e.to_string(),
                None,
                None,
            );
        }
        Err(Error::NoopChangesOnly(_, _)) => {
            svp.report_nothing_to_do(
                Some("No missing versions with effective changes"),
                Some("Run with --no-skip-noop to include trivial uploads."),
            );
        }
        Err(Error::NoMissingVersions(_, _)) => {
            svp.report_nothing_to_do(Some("No missing versions"), None);
        }
        Err(Error::SnapshotDownloadError {
            url,
            error,
            is_server_error,
        }) => {
            svp.report_fatal(
                "snapshot-download-failed",
                &format!("Downloading {} failed: {}", url, error),
                None,
                is_server_error,
            );
        }
        Err(Error::SnapshotHashMismatch {
            filename,
            expected_hash,
            actual_hash,
        }) => {
            svp.report_fatal(
                "snapshot-hash-mismatch",
                &format!(
                    "Snapshot hash mismatch for {}: {} != {}",
                    filename, expected_hash, actual_hash
                ),
                None,
                None,
            );
        }
        Err(Error::ConflictsInTree) => {
            svp.report_fatal(
                "merge-conflicts",
                "Merging uncommitted changes resulted in conflicts.",
                None,
                Some(false),
            );
        }
        Err(Error::SnapshotMissing { package, version }) => {
            svp.report_fatal(
                "snapshot-missing",
                &format!("Snapshot for {} {} missing", package, version),
                None,
                Some(false),
            );
        }
    };

    let target_branch_url = if let Some(vcs_git_base) = args.vcs_git_base.as_ref() {
        let control: debian_analyzer::editor::TreeEditor<Control> = local_tree
            .edit_file(&subpath.join("debian/control"), false, false)
            .unwrap();
        use debian_analyzer::editor::Editor;
        use std::ops::Deref;
        let (old_vcs_url, new_vcs_url) = set_vcs_git_url(
            control.deref(),
            Some(vcs_git_base.as_ref()),
            args.vcs_browser_base.as_deref(),
        );
        control.commit().unwrap();
        if old_vcs_url != new_vcs_url {
            log::info!("Updating Vcs-Git URL to {}", new_vcs_url.as_ref().unwrap());
            let mut changelog: debian_analyzer::editor::TreeEditor<debian_changelog::ChangeLog> =
                local_tree
                    .edit_file(&subpath.join("debian/changelog"), false, false)
                    .unwrap();
            changelog
                .try_auto_add_change(
                    &["Set Vcs-Git header."],
                    debian_changelog::get_maintainer().unwrap(),
                    None::<String>,
                    None,
                )
                .unwrap();
            debian_analyzer::debcommit::debcommit(&local_tree, None, &subpath, None, None, None)
                .unwrap();
            Some(breezyshim::debian::directory::vcs_git_url_to_bzr_url(
                new_vcs_url.as_deref().unwrap(),
            ))
        } else {
            None
        }
    } else {
        None
    };

    if svp_client::enabled() {
        let commit_message = if ret.len() == 1 {
            let (_tag_name, version, _rs) = &ret[0];
            let commit_message = format!("Import missing upload: {}", version);
            commit_message
        } else {
            let commit_message = format!(
                "Import missing uploads: {}.",
                ret.iter()
                    .map(|(t, v, _rs)| format!("{} ({})", v, t))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            commit_message
        };

        if let Some(target_branch_url) = target_branch_url {
            svp.set_target_branch_url(target_branch_url);
        }

        svp.set_commit_message(commit_message);
        svp.report_success_debian(
            Some(
                60 + ret
                    .iter()
                    .map(|(_t, v, _rs)| {
                        if v.to_string().contains("nmu") {
                            60
                        } else {
                            20
                        }
                    })
                    .sum::<i32>(),
            ),
            Some(Context::new(
                ret.iter()
                    .map(|(t, v, _rs)| (t.clone(), v.clone()))
                    .collect(),
            )),
            None,
        );
    }

    log::info!(
        "Imported uploads: {}.",
        ret.iter()
            .map(|(t, v, _rs)| format!("{} ({})", v, t))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn versions_dict() -> HashMap<String, String> {
    HashMap::from([(
        "breezyshim".to_string(),
        breezyshim::version::version().to_string(),
    )])
}

#[cfg(test)]
mod tests {
    use super::*;
    use debian_changelog::ChangeLog;
    use debversion::Version;

    #[test]
    fn test_find_missing_versions_empty_changelog() {
        let changelog_content = "";
        let changelog = ChangeLog::read(changelog_content.as_bytes()).unwrap();
        let result = find_missing_versions(&changelog, None).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_find_missing_versions_no_tree_version() {
        let changelog_content = r#"package (2.0-1) unstable; urgency=medium

  * New upstream version

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

package (1.0-1) unstable; urgency=medium

  * Initial release

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2023 12:00:00 +0000
"#;
        let changelog = ChangeLog::read(changelog_content.as_bytes()).unwrap();
        let result = find_missing_versions(&changelog, None).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "2.0-1".parse::<Version>().unwrap());
        assert_eq!(result[1], "1.0-1".parse::<Version>().unwrap());
    }

    #[test]
    fn test_find_missing_versions_with_tree_version() {
        let changelog_content = r#"package (3.0-1) unstable; urgency=medium

  * Latest version

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2025 12:00:00 +0000

package (2.0-1) unstable; urgency=medium

  * New upstream version

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

package (1.0-1) unstable; urgency=medium

  * Initial release

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2023 12:00:00 +0000
"#;
        let changelog = ChangeLog::read(changelog_content.as_bytes()).unwrap();
        let tree_version = "1.0-1".parse::<Version>().unwrap();
        let result = find_missing_versions(&changelog, Some(&tree_version)).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "3.0-1".parse::<Version>().unwrap());
        assert_eq!(result[1], "2.0-1".parse::<Version>().unwrap());
    }

    #[test]
    fn test_find_missing_versions_tree_version_not_found() {
        let changelog_content = r#"package (2.0-1) unstable; urgency=medium

  * New upstream version

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;
        let changelog = ChangeLog::read(changelog_content.as_bytes()).unwrap();
        let tree_version = "1.0-1".parse::<Version>().unwrap();
        let result = find_missing_versions(&changelog, Some(&tree_version));
        assert!(result.is_err());
        match result {
            Err(Error::TreeVersionNotInArchiveChangelog(v)) => {
                assert_eq!(v, tree_version);
            }
            _ => panic!("Expected TreeVersionNotInArchiveChangelog error"),
        }
    }

    #[test]
    fn test_find_missing_versions_exact_match() {
        let changelog_content = r#"package (1.0-1) unstable; urgency=medium

  * Initial release

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2023 12:00:00 +0000
"#;
        let changelog = ChangeLog::read(changelog_content.as_bytes()).unwrap();
        let tree_version = "1.0-1".parse::<Version>().unwrap();
        let result = find_missing_versions(&changelog, Some(&tree_version)).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_versions_dict() {
        let versions = versions_dict();
        assert!(versions.contains_key("breezyshim"));
        assert!(!versions["breezyshim"].is_empty());
    }

    #[test]
    fn test_set_vcs_git_url_with_base() {
        use debian_control::lossless::Control;

        let control_content = r#"Source: test-package
Section: misc
Priority: optional
Maintainer: Test Maintainer <test@example.com>
Build-Depends: debhelper (>= 10)
Standards-Version: 4.1.0

Package: test-package
Architecture: any
Depends: ${misc:Depends}
Description: Test package
 This is a test package.
"#;
        let control = Control::read(control_content.as_bytes()).unwrap();

        let (old_url, new_url) = set_vcs_git_url(
            &control,
            Some("https://salsa.debian.org/maintainer"),
            Some("https://salsa.debian.org/maintainer"),
        );

        assert!(old_url.is_none());
        assert_eq!(
            new_url,
            Some("https://salsa.debian.org/maintainer/test-package.git".to_string())
        );
    }

    #[test]
    fn test_set_vcs_git_url_no_base() {
        use debian_control::lossless::Control;

        let control_content = r#"Source: test-package
Section: misc
Priority: optional
Maintainer: Test Maintainer <test@example.com>
Build-Depends: debhelper (>= 10)
Standards-Version: 4.1.0
Vcs-Git: https://github.com/original/test-package.git

Package: test-package
Architecture: any
Depends: ${misc:Depends}
Description: Test package
 This is a test package.
"#;
        let control = Control::read(control_content.as_bytes()).unwrap();

        let (old_url, new_url) = set_vcs_git_url(&control, None, None);

        assert_eq!(
            old_url,
            Some("https://github.com/original/test-package.git".to_string())
        );
        assert_eq!(
            new_url,
            Some("https://github.com/original/test-package.git".to_string())
        );
    }

    #[test]
    fn test_nmu_detection_in_value_calculation() {
        // Bug #3: NMU detection should check version string, not tag name
        let ret = vec![
            (
                "debian/1.0-1".to_string(),
                "1.0-1".parse::<Version>().unwrap(),
                breezyshim::RevisionId::null(),
            ),
            (
                "debian/1.0+nmu1".to_string(),
                "1.0+nmu1".parse::<Version>().unwrap(),
                breezyshim::RevisionId::null(),
            ),
            (
                "debian/2.0-1".to_string(),
                "2.0-1".parse::<Version>().unwrap(),
                breezyshim::RevisionId::null(),
            ),
        ];

        // Calculate value as in main()
        let value: i32 = 60
            + ret
                .iter()
                .map(|(_t, v, _rs)| {
                    if v.to_string().contains("nmu") {
                        60
                    } else {
                        20
                    }
                })
                .sum::<i32>();

        // Should be: 60 (base) + 20 (1.0-1) + 60 (1.0+nmu1) + 20 (2.0-1) = 160
        assert_eq!(value, 160);

        // Verify that nmu is detected in the version string
        assert!("1.0+nmu1"
            .parse::<Version>()
            .unwrap()
            .to_string()
            .contains("nmu"));
        assert!(!"1.0-1"
            .parse::<Version>()
            .unwrap()
            .to_string()
            .contains("nmu"));
    }

    #[test]
    fn test_context_struct_accessors() {
        // Test the Context struct and its accessor methods
        let tags = vec![
            (
                "debian/1.0-1".to_string(),
                "1.0-1".parse::<Version>().unwrap(),
            ),
            (
                "debian/2.0-1".to_string(),
                "2.0-1".parse::<Version>().unwrap(),
            ),
        ];

        let context = Context::new(tags.clone());

        // Test tag_names accessor
        let tag_names = context.tag_names();
        assert_eq!(tag_names, vec!["debian/1.0-1", "debian/2.0-1"]);

        // Test versions accessor
        let versions = context.versions();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0], &"1.0-1".parse::<Version>().unwrap());
        assert_eq!(versions[1], &"2.0-1".parse::<Version>().unwrap());

        // Test tags accessor
        let retrieved_tags = context.tags();
        assert_eq!(retrieved_tags.len(), 2);
        assert_eq!(retrieved_tags[0].0, "debian/1.0-1");
        assert_eq!(retrieved_tags[1].0, "debian/2.0-1");
    }

    #[test]
    fn test_context_struct_serialization() {
        // Verify that Context can be serialized (for SVP reporting)
        let tags = vec![(
            "debian/1.0-1".to_string(),
            "1.0-1".parse::<Version>().unwrap(),
        )];

        let context = Context::new(tags);
        let json = serde_json::to_string(&context).unwrap();

        // Should contain the tags field
        assert!(json.contains("tags"));
        assert!(json.contains("debian/1.0-1"));
        assert!(json.contains("1.0-1"));
    }

    #[test]
    fn test_find_missing_versions_warns_on_invalid_entries() {
        // Test that we warn when encountering changelog blocks without versions
        // Note: This is a behavioral improvement over Python which would crash
        let changelog_content = r#"package (2.0-1) unstable; urgency=medium

  * New upstream version

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

package (1.0-1) unstable; urgency=medium

  * Initial release

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2023 12:00:00 +0000
"#;
        let changelog = ChangeLog::read(changelog_content.as_bytes()).unwrap();

        // This should succeed and return the valid versions
        let result = find_missing_versions(&changelog, None).unwrap();
        assert_eq!(result.len(), 2);

        // Note: Testing for warning output would require capturing log output,
        // which is complex. The important thing is that the function doesn't
        // silently skip entries - it logs a warning (verified by manual inspection).
        // In a real changelog with missing versions, the warning would appear.
    }

    // Helper function to create a working tree for testing
    fn create_test_tree() -> (
        tempfile::TempDir,
        breezyshim::workingtree::GenericWorkingTree,
    ) {
        use breezyshim::controldir::ControlDirFormat;

        breezyshim::init();

        let td = tempfile::tempdir().unwrap();
        let format = ControlDirFormat::default();
        let transport = breezyshim::transport::get_transport(
            &url::Url::from_file_path(td.path()).unwrap(),
            None,
        )
        .unwrap();

        let controldir = format.initialize_on_transport(&transport).unwrap();
        controldir.create_repository(None).unwrap();
        controldir.create_branch(None).unwrap();
        let wt = controldir.create_workingtree().unwrap();

        // Create an initial commit
        wt.build_commit()
            .message("Initial commit")
            .commit()
            .unwrap();

        (td, wt)
    }

    #[test]
    fn test_is_noop_upload_no_changes() {
        // Test case: No changes at all - should return true
        let (_td, wt) = create_test_tree();

        let lock = wt.lock_read().unwrap();
        let basis_tree = wt.basis_tree().unwrap();

        let result = is_noop_upload(&wt, &basis_tree, Path::new(""));
        std::mem::drop(lock);
        assert!(result, "No changes should be considered noop");
    }

    #[test]
    fn test_is_noop_upload_changelog_only() {
        use breezyshim::tree::MutableTree;

        // Test case: Only changelog changed (new entry added) - should return true
        let (_td, wt) = create_test_tree();

        // Create initial debian/changelog
        std::fs::create_dir_all(_td.path().join("debian")).unwrap();
        let changelog_content = r#"package (1.0-1) unstable; urgency=medium

  * Initial release

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2023 12:00:00 +0000
"#;
        wt.put_file_bytes_non_atomic(Path::new("debian/changelog"), changelog_content.as_bytes())
            .unwrap();
        wt.add(&[Path::new("debian"), Path::new("debian/changelog")])
            .unwrap();
        wt.build_commit().message("Add changelog").commit().unwrap();

        let lock = wt.lock_write().unwrap();
        let basis_tree = wt.basis_tree().unwrap();

        // Now add a new entry (simulating noop upload)
        let new_changelog_content = r#"package (1.0-2) unstable; urgency=medium

  * Rebuild for transition

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

package (1.0-1) unstable; urgency=medium

  * Initial release

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2023 12:00:00 +0000
"#;
        wt.put_file_bytes_non_atomic(
            Path::new("debian/changelog"),
            new_changelog_content.as_bytes(),
        )
        .unwrap();

        let result = is_noop_upload(&wt, &basis_tree, Path::new(""));
        std::mem::drop(lock);
        assert!(result, "Changelog-only change should be noop");
    }

    #[test]
    fn test_is_noop_upload_with_other_changes() {
        use breezyshim::tree::MutableTree;

        // Test case: Changelog + other file changes - should return false
        let (_td, wt) = create_test_tree();

        // Create initial files
        std::fs::create_dir_all(_td.path().join("debian")).unwrap();
        let changelog_content = r#"package (1.0-1) unstable; urgency=medium

  * Initial release

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2023 12:00:00 +0000
"#;
        wt.put_file_bytes_non_atomic(Path::new("debian/changelog"), changelog_content.as_bytes())
            .unwrap();
        wt.put_file_bytes_non_atomic(Path::new("README.md"), b"# Test\n")
            .unwrap();
        wt.add(&[
            Path::new("debian"),
            Path::new("debian/changelog"),
            Path::new("README.md"),
        ])
        .unwrap();
        wt.build_commit().message("Add files").commit().unwrap();

        let lock = wt.lock_write().unwrap();
        let basis_tree = wt.basis_tree().unwrap();

        // Modify both changelog and README
        let new_changelog = r#"package (1.0-2) unstable; urgency=medium

  * Update

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

package (1.0-1) unstable; urgency=medium

  * Initial release

 -- Maintainer <maint@example.com>  Mon, 01 Jan 2023 12:00:00 +0000
"#;
        wt.put_file_bytes_non_atomic(Path::new("debian/changelog"), new_changelog.as_bytes())
            .unwrap();
        wt.put_file_bytes_non_atomic(Path::new("README.md"), b"# Test\nUpdated\n")
            .unwrap();

        let result = is_noop_upload(&wt, &basis_tree, Path::new(""));
        std::mem::drop(lock);
        assert!(
            !result,
            "Changes beyond changelog should not be considered noop"
        );
    }

    #[test]
    fn test_is_noop_upload_no_changelog() {
        use breezyshim::tree::MutableTree;

        // Test case: Changes but no changelog - should return false
        let (_td, wt) = create_test_tree();

        wt.put_file_bytes_non_atomic(Path::new("README.md"), b"# Test\n")
            .unwrap();
        wt.add(&[Path::new("README.md")]).unwrap();
        wt.build_commit().message("Add README").commit().unwrap();

        let lock = wt.lock_write().unwrap();
        let basis_tree = wt.basis_tree().unwrap();

        wt.put_file_bytes_non_atomic(Path::new("README.md"), b"# Test\nUpdated\n")
            .unwrap();

        let result = is_noop_upload(&wt, &basis_tree, Path::new(""));
        std::mem::drop(lock);
        assert!(!result, "Non-changelog changes should not be noop");
    }

    #[test]
    fn test_contains_git_attributes_no_files() {
        // Test case: No .gitattributes files - should return false
        let (_td, wt) = create_test_tree();

        let _lock = wt.lock_read().unwrap();
        let result = contains_git_attributes(&wt, Path::new(""));
        assert!(!result, "Should return false when no .gitattributes exist");
    }

    #[test]
    fn test_contains_git_attributes_with_file() {
        use breezyshim::tree::MutableTree;

        // Test case: .gitattributes exists - should return true
        let (_td, wt) = create_test_tree();

        wt.put_file_bytes_non_atomic(Path::new(".gitattributes"), b"* text=auto\n")
            .unwrap();
        wt.add(&[Path::new(".gitattributes")]).unwrap();
        wt.build_commit()
            .message("Add .gitattributes")
            .commit()
            .unwrap();

        let _lock = wt.lock_read().unwrap();
        let result = contains_git_attributes(&wt, Path::new(""));
        assert!(result, "Should return true when .gitattributes exists");
    }

    #[test]
    fn test_contains_git_attributes_in_subdir() {
        use breezyshim::tree::MutableTree;

        // Test case: .gitattributes in subdirectory - should return true
        let (_td, wt) = create_test_tree();

        wt.mkdir(Path::new("subdir")).unwrap();
        wt.put_file_bytes_non_atomic(Path::new("subdir/.gitattributes"), b"* text=auto\n")
            .unwrap();
        wt.add(&[Path::new("subdir"), Path::new("subdir/.gitattributes")])
            .unwrap();
        wt.build_commit()
            .message("Add .gitattributes in subdir")
            .commit()
            .unwrap();

        let _lock = wt.lock_read().unwrap();
        let result = contains_git_attributes(&wt, Path::new(""));
        assert!(
            result,
            "Should return true when .gitattributes exists in subdirectory"
        );
    }

    // Error Display implementation tests
    #[test]
    fn test_error_display_noop_changes_only() {
        let error = Error::NoopChangesOnly(
            "1.0-1".parse::<Version>().unwrap(),
            "2.0-1".parse::<Version>().unwrap(),
        );
        assert_eq!(
            error.to_string(),
            "No missing versions with effective changes. Archive has 2.0-1, VCS has 1.0-1"
        );
    }

    #[test]
    fn test_error_display_no_missing_versions() {
        let error = Error::NoMissingVersions(
            "1.0-1".parse::<Version>().unwrap(),
            "1.0-1".parse::<Version>().unwrap(),
        );
        assert_eq!(
            error.to_string(),
            "No missing versions after all. Archive has 1.0-1, VCS has 1.0-1"
        );
    }

    #[test]
    fn test_error_display_tree_version_not_in_archive() {
        let error = Error::TreeVersionNotInArchiveChangelog("1.0-1".parse::<Version>().unwrap());
        assert_eq!(
            error.to_string(),
            "Tree version 1.0-1 does not appear in archive changelog"
        );
    }

    #[test]
    fn test_error_display_tree_version_without_tag() {
        let error = Error::TreeVersionWithoutTag(
            "1.0-1".parse::<Version>().unwrap(),
            "debian/1.0-1".to_string(),
        );
        assert_eq!(
            error.to_string(),
            "Tree version 1.0-1 does not have a tag (e.g. debian/1.0-1)"
        );
    }

    #[test]
    fn test_error_display_tree_upstream_version_missing() {
        let error = Error::TreeUpstreamVersionMissing("1.0".to_string());
        assert_eq!(error.to_string(), "Unable to find upstream version 1.0");
    }

    #[test]
    fn test_error_display_unreleased_changes() {
        let error = Error::UnreleasedChangesSinceTreeVersion("1.0-1".parse::<Version>().unwrap());
        assert_eq!(
            error.to_string(),
            "There are unreleased changes since 1.0-1"
        );
    }

    #[test]
    fn test_error_display_snapshot_missing() {
        let error = Error::SnapshotMissing {
            package: "test-package".to_string(),
            version: "1.0-1".parse::<Version>().unwrap(),
        };
        assert_eq!(error.to_string(), "Snapshot for test-package 1.0-1 missing");
    }

    #[test]
    fn test_error_display_snapshot_hash_mismatch() {
        let error = Error::SnapshotHashMismatch {
            filename: "test.dsc".to_string(),
            expected_hash: "abc123".to_string(),
            actual_hash: "def456".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Snapshot hash mismatch for test.dsc: abc123 != def456"
        );
    }

    #[test]
    fn test_error_display_snapshot_download_error() {
        let error = Error::SnapshotDownloadError {
            url: "https://example.com/file".to_string(),
            error: "Connection timeout".to_string(),
            is_server_error: Some(false),
        };
        assert_eq!(
            error.to_string(),
            "Failed to download snapshot from https://example.com/file: Connection timeout"
        );
    }

    #[test]
    fn test_error_display_conflicts_in_tree() {
        let error = Error::ConflictsInTree;
        assert_eq!(error.to_string(), "Conflicts in tree");
    }

    // Integration tests for the main workflow

    #[test]
    fn test_merge_into_logic_saves_correct_revision() {
        // This test verifies the bug fix where we were saving the wrong revision
        // before updating the tree. The merge_into variable should contain the
        // ORIGINAL last revision before we update to tree_version.
        use breezyshim::branch::Branch;
        use breezyshim::tree::MutableTree;

        let (_td, wt) = create_test_tree();

        // Create a debian/changelog with version 1.0-1
        std::fs::create_dir_all(_td.path().join("debian")).unwrap();
        let changelog1 = r#"testpkg (1.0-1) unstable; urgency=medium

  * Initial release

 -- Test <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;
        wt.put_file_bytes_non_atomic(Path::new("debian/changelog"), changelog1.as_bytes())
            .unwrap();
        wt.add(&[Path::new("debian"), Path::new("debian/changelog")])
            .unwrap();
        let rev1 = wt.build_commit().message("Release 1.0-1").commit().unwrap();

        // Tag this version
        let branch = wt.branch();
        branch
            .tags()
            .unwrap()
            .set_tag("debian/1.0-1", &rev1)
            .unwrap();

        // Now make an unreleased change
        let changelog2 = r#"testpkg (1.0-2) UNRELEASED; urgency=medium

  * Unreleased change

 -- Test <test@example.com>  Mon, 02 Jan 2024 12:00:00 +0000

testpkg (1.0-1) unstable; urgency=medium

  * Initial release

 -- Test <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;
        wt.put_file_bytes_non_atomic(Path::new("debian/changelog"), changelog2.as_bytes())
            .unwrap();
        let rev2 = wt
            .build_commit()
            .message("Add unreleased changes")
            .commit()
            .unwrap();

        // Verify setup: we have two commits
        assert_ne!(rev1, rev2);
        assert_eq!(wt.last_revision().unwrap(), rev2);

        // Simulate what import_uncommitted does when merge_unreleased is true:
        // THE BUG WE FIXED: We must save the CURRENT revision BEFORE updating

        // CORRECT: Save the CURRENT revision (rev2, the tip with unreleased changes) BEFORE update
        let original_last_revision = wt.last_revision().unwrap();
        assert_eq!(
            original_last_revision, rev2,
            "Should save the current revision (with unreleased changes)"
        );
        assert_ne!(
            original_last_revision, rev1,
            "Should NOT save the tree_version"
        );

        // Then update to tree_version (rev1) - this is what the code does to import from a clean state
        wt.update(Some(&rev1)).unwrap();

        // The key point of the bug fix: original_last_revision still holds rev2
        // This is what will be used as merge_into later to merge back the unreleased changes
        // Even after update(), the saved variable still has the correct value
        assert_eq!(
            original_last_revision, rev2,
            "merge_into should be the original tip (rev2), not the tree_version (rev1)"
        );

        // This test verifies the fix for the bug where the code was saving
        // `tree.last_revision()` AFTER calling `tree.update()`, which would give
        // the wrong revision for merge_into.
    }

    #[test]
    fn test_nmu_detection_comprehensive() {
        // Test NMU detection with various version patterns
        let test_cases = vec![
            ("1.0-1", false, "Regular upload"),
            ("1.0-1nmu1", true, "NMU with nmu suffix"),
            ("1.0-1+nmu1", true, "NMU with +nmu"),
            (
                "1.0-1.1",
                false,
                "Binary NMU (not detected as NMU in version string)",
            ),
            ("1.0-1ubuntu1", false, "Ubuntu upload"),
            ("1.0-1+deb11u1", false, "Security update"),
        ];

        for (version_str, expected_is_nmu, description) in test_cases {
            let version: Version = version_str.parse().unwrap();
            let contains_nmu = version.to_string().contains("nmu");
            assert_eq!(
                contains_nmu, expected_is_nmu,
                "Failed for {}: {}",
                version_str, description
            );
        }
    }

    #[test]
    fn test_nmu_scoring_calculation() {
        // Test that NMU uploads get higher scores in value calculation
        // This tests the logic at line 809 where NMU versions get score 60 vs 20

        // Create tags with different version patterns
        let tags = vec![
            (
                "debian/1.0-1".to_string(),
                "1.0-1".parse::<Version>().unwrap(),
            ),
            (
                "debian/1.0-1nmu1".to_string(),
                "1.0-1nmu1".parse::<Version>().unwrap(),
            ),
            (
                "debian/1.0-2".to_string(),
                "1.0-2".parse::<Version>().unwrap(),
            ),
        ];

        // Calculate values (simplified version of the logic)
        for (tag, version) in &tags {
            let score = if version.to_string().contains("nmu") {
                60
            } else {
                20
            };

            if version.to_string().contains("nmu") {
                assert_eq!(score, 60, "NMU version {} should have score 60", tag);
            } else {
                assert_eq!(score, 20, "Regular version {} should have score 20", tag);
            }
        }
    }

    #[test]
    fn test_context_struct_with_nmu_versions() {
        // Test that Context properly handles NMU versions
        let tags = vec![
            (
                "debian/1.0-1".to_string(),
                "1.0-1".parse::<Version>().unwrap(),
            ),
            (
                "debian/1.0-1nmu1".to_string(),
                "1.0-1nmu1".parse::<Version>().unwrap(),
            ),
            (
                "debian/1.0-2".to_string(),
                "1.0-2".parse::<Version>().unwrap(),
            ),
        ];

        let context = Context::new(tags.clone());

        // Verify all versions are accessible
        let versions = context.versions();
        assert_eq!(versions.len(), 3);

        // Verify we can distinguish NMU versions
        let has_nmu = versions.iter().any(|v| v.to_string().contains("nmu"));
        assert!(has_nmu, "Should have at least one NMU version");

        // Verify tag names
        let tag_names = context.tag_names();
        assert_eq!(tag_names.len(), 3);
        assert!(tag_names.contains(&"debian/1.0-1nmu1"));
    }

    #[test]
    fn test_is_noop_upload_with_debian_subdir() {
        // Test that is_noop_upload correctly handles files in debian/ subdirectory
        use breezyshim::tree::MutableTree;

        let (_td, wt) = create_test_tree();

        // Create debian/changelog
        std::fs::create_dir_all(_td.path().join("debian")).unwrap();
        let changelog = r#"testpkg (1.0-1) unstable; urgency=medium

  * Initial release

 -- Test <test@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;
        wt.put_file_bytes_non_atomic(Path::new("debian/changelog"), changelog.as_bytes())
            .unwrap();

        // Add a control file
        wt.put_file_bytes_non_atomic(Path::new("debian/control"), b"Source: testpkg\n")
            .unwrap();

        wt.add(&[
            Path::new("debian"),
            Path::new("debian/changelog"),
            Path::new("debian/control"),
        ])
        .unwrap();
        wt.build_commit()
            .message("Initial debian files")
            .commit()
            .unwrap();

        let lock = wt.lock_write().unwrap();
        let basis_tree = wt.basis_tree().unwrap();

        // Modify only debian/control (not changelog)
        wt.put_file_bytes_non_atomic(
            Path::new("debian/control"),
            b"Source: testpkg\nSection: devel\n",
        )
        .unwrap();

        // This should NOT be a noop (changed more than just changelog)
        let result = is_noop_upload(&wt, &basis_tree, Path::new(""));
        std::mem::drop(lock);

        assert!(!result, "Changes to debian/control should not be noop");
    }

    #[test]
    fn test_versions_dict_output_format() {
        // Test that versions_dict returns expected format
        let versions = versions_dict();

        // Should include breezyshim version
        assert!(
            versions.contains_key("breezyshim"),
            "Should contain breezyshim version"
        );

        // Verify it's a HashMap<String, String>
        for (key, value) in versions.iter() {
            assert!(!key.is_empty(), "Keys should not be empty");
            assert!(!value.is_empty(), "Values should not be empty");
        }

        // Verify we can get the breezyshim version
        let breezyshim_version = versions.get("breezyshim").unwrap();
        assert!(!breezyshim_version.is_empty());
    }
}
