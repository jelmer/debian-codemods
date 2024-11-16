use breezyshim::debian::apt::{Apt, LocalApt, RemoteApt};
use breezyshim::debian::error::Error as DebianError;
use breezyshim::debian::import_dsc::{DistributionBranch, DistributionBranchSet};
use breezyshim::debian::upstream::UpstreamSource;
use breezyshim::error::Error as BrzError;
use breezyshim::tree::Tree;
use breezyshim::workingtree::WorkingTree;
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
    for block in archive_cl.iter() {
        if tree_version.is_some() && block.version().as_ref() == tree_version {
            found = true;
            break;
        }
        if let Some(version) = block.version() {
            missing_versions.push(version);
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

fn is_noop_upload(tree: &WorkingTree, basis_tree: &dyn Tree, subpath: &Path) -> bool {
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

    // TODO(jelmer): Check for uploads that aren't just meant to trigger a build.  i.e. closing
    // bugs.
    new_cl == old_cl
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
            vcs_git.repo_url.trim_end_matches("/"),
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
    for entry in tree
        .list_files(None, Some(subpath), Some(true), Some(true))
        .unwrap()
    {
        let entry = entry.unwrap();
        if entry.0.file_name() == Some(std::ffi::OsStr::new(".gitattributes")) {
            return true;
        }
    }
    false
}

fn import_uncommitted(
    tree: &WorkingTree,
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
        .next()
        .unwrap()
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
    let db = DistributionBranch::new(
        tree.branch().as_ref(),
        tree.branch().as_ref(),
        Some(tree),
        None,
    );
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

            tree.update(Some(&tree_version_revid)).unwrap();
            Some(tree.last_revision().unwrap())
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
        match tree.merge_from_branch(tree.branch().as_ref(), Some(&to_merge)) {
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
    #[clap(long, env = "APT_REPOSITORY", env = "REPOSITORIES")]
    /// APT repository to use. Defaults to locally configured.
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
    versions: Vec<debversion::Version>,
    tags: Vec<(String, debversion::Version)>,
}

pub fn main() {
    use std::io::Write;
    let args = Args::parse();

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
            changelog.auto_add_change(
                &["Set Vcs-Git header."],
                debian_changelog::get_maintainer().unwrap(),
                None,
                None,
            );
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
                    .map(|(t, _v, _rs)| if t.contains("nmu") { 60 } else { 20 })
                    .sum::<i32>(),
            ),
            Some(Context {
                versions: ret.iter().map(|(_t, v, _rs)| v.clone()).collect(),
                tags: ret
                    .iter()
                    .map(|(t, v, _rs)| (t.clone(), v.clone()))
                    .collect(),
            }),
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
    let mut versions = HashMap::new();
    versions.insert(
        "breezyshim".to_string(),
        breezyshim::version::version().to_string(),
    );
    versions
}
