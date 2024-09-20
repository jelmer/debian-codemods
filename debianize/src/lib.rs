use breezyshim::branch::Branch;
use breezyshim::debian::error::Error as BrzDebianError;
use breezyshim::debian::merge_upstream::{
    do_import, get_existing_imported_upstream_revids, get_tarballs,
};
use breezyshim::debian::upstream::{
    upstream_version_add_revision, PristineTarSource, UpstreamBranchSource, UpstreamSource,
};
use breezyshim::debian::{TarballKind, VersionKind, DEFAULT_ORIG_DIR};
use breezyshim::error::Error as BrzError;
use breezyshim::tree::Tree;
use breezyshim::workingtree::WorkingTree;
use breezyshim::RevisionId;
use debian_analyzer::versions::debianize_upstream_version;
use debian_analyzer::wnpp::{find_wnpp_bugs_harder, BugKind};
use debian_analyzer::Certainty;
use debversion::Version;
use ognibuild::dependencies::debian::valid_debian_package_name;
use ognibuild::dependencies::debian::DebianDependency;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use upstream_ontologist::{
    guess_upstream_info, summarize_upstream_metadata, ProviderError, UpstreamMetadata,
};

pub mod fixer;
pub mod names;
pub mod processors;
pub mod simple_apt_repo;

pub fn default_debianize_cache_dir() -> std::io::Result<std::path::PathBuf> {
    xdg::BaseDirectories::with_prefix("debianize")?.create_cache_directory("")
}

pub fn write_changelog_template(
    path: &std::path::Path,
    source_name: &str,
    version: &Version,
    author: Option<(String, String)>,
    wnpp_bugs: Vec<(i64, BugKind)>,
) -> Result<(), std::io::Error> {
    let author = author.unwrap_or_else(|| debian_changelog::get_maintainer().unwrap());
    let closes = if wnpp_bugs.len() > 0 {
        format!(
            " Closes: {}",
            wnpp_bugs
                .iter()
                .map(|(n, _k)| format!("#{}", n))
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        "".to_string()
    };
    let mut cl = debian_changelog::ChangeLog::new();

    cl.new_entry()
        .package(source_name.to_string())
        .version(version.clone())
        .distribution("UNRELEASED".to_string())
        .urgency(debian_changelog::Urgency::Low)
        .change_line(format!("  * Initial release.{}", closes))
        .maintainer(author)
        .finish();

    let buf = cl.to_string();

    std::fs::write(path, buf)?;

    Ok(())
}

pub fn use_packaging_branch(wt: &WorkingTree, branch_name: &str) -> Result<(), BrzError> {
    let last_revision = wt.last_revision()?;
    let target_branch = match wt.controldir().open_branch(Some(branch_name)) {
        Ok(b) => b,
        Err(BrzError::NotBranchError { .. }) => wt.controldir().create_branch(Some(branch_name))?,
        Err(e) => return Err(e),
    };

    target_branch.generate_revision_history(&last_revision)?;
    log::info!("Switching to packaging branch {}.", branch_name);
    wt.controldir()
        .set_branch_reference(target_branch.as_ref(), Some(""))?;
    // TODO(jelmer): breezy bug?
    pyo3::Python::with_gil(|py| -> pyo3::PyResult<()> {
        use pyo3::ToPyObject;
        let wt = wt.to_object(py);
        wt.setattr(py, "_branch", target_branch.to_object(py))?;
        Ok(())
    })
    .unwrap();
    Ok(())
}

pub fn import_upstream_version_from_dist(
    wt: &WorkingTree,
    subpath: &std::path::Path,
    upstream_source: &UpstreamBranchSource,
    source_name: &str,
    upstream_version: &str,
) -> Result<
    (
        HashMap<TarballKind, (RevisionId, PathBuf)>,
        HashMap<TarballKind, String>,
        String,
    ),
    BrzDebianError,
> {
    let orig_dir = Path::new(DEFAULT_ORIG_DIR).canonicalize().unwrap();

    let mut tag_names = HashMap::new();
    let td = tempfile::tempdir().unwrap();
    let locations = upstream_source.fetch_tarballs(
        Some(source_name),
        upstream_version,
        td.path(),
        Some(&[TarballKind::Orig]),
    )?;
    let tarball_filenames = match get_tarballs(
        &orig_dir,
        wt,
        source_name,
        upstream_version,
        locations
            .iter()
            .map(|x| x.as_ref())
            .collect::<Vec<_>>()
            .as_slice(),
    ) {
        Ok(filenames) => filenames,
        Err(BrzDebianError::BrzError(BrzError::FileExists(path, _))) => {
            log::warn!("Tarball {} exists, reusing existing file.", path.display());
            vec![orig_dir.join(path)]
        }
        Err(e) => return Err(e),
    };
    let upstream_revisions =
        upstream_source.version_as_revisions(Some(source_name), upstream_version, None)?;
    let files_excluded = None;
    let imported_revids = match do_import(
        wt,
        subpath,
        tarball_filenames
            .iter()
            .map(|x| x.as_path())
            .collect::<Vec<_>>()
            .as_slice(),
        &source_name,
        upstream_version,
        None,
        upstream_source.upstream_branch().as_ref(),
        upstream_revisions,
        None,
        false,
        false,
        None,
        files_excluded,
    ) {
        Ok(revids) => revids,
        Err(BrzDebianError::UpstreamAlreadyImported(version)) => {
            log::warn!("Upstream release {} already imported.", version);
            get_existing_imported_upstream_revids(upstream_source, source_name, upstream_version)?
        }
        Err(e) => return Err(e),
    };
    let mut pristine_revids = HashMap::new();
    for (component, tag_name, revid, _pristine_tar_imported, subpath) in imported_revids {
        pristine_revids.insert(component.clone(), (revid, subpath));
        tag_names.insert(component, tag_name);
    }
    std::mem::drop(td);

    let upstream_branch_name = "upstream";
    match wt.controldir().create_branch(Some(upstream_branch_name)) {
        Ok(branch) => {
            branch
                .generate_revision_history(&pristine_revids.get(&TarballKind::Orig).unwrap().0)?;
            log::info!("Created upstream branch.");
        }
        Err(BrzError::AlreadyBranch(..)) => {
            log::info!("Upstream branch already exists; not creating.");
            wt.controldir().open_branch(Some(upstream_branch_name))?;
        }
        Err(e) => return Err(e.into()),
    }

    Ok((pristine_revids, tag_names, upstream_branch_name.to_string()))
}

pub fn import_upstream_dist(
    pristine_tar_source: &PristineTarSource,
    wt: &WorkingTree,
    upstream_source: &UpstreamBranchSource,
    subpath: &Path,
    source_name: &str,
    upstream_version: &str,
) -> Result<(RevisionId, Option<String>, HashMap<TarballKind, String>), BrzDebianError> {
    let (mut pristine_revids, tag_names, upstream_branch_name) = if pristine_tar_source
        .has_version(Some(source_name), upstream_version, None, false)?
    {
        log::warn!(
            "Upstream version {}/{} already imported.",
            source_name,
            upstream_version,
        );
        let pristine_revids =
            pristine_tar_source.version_as_revisions(Some(source_name), upstream_version, None)?;
        let upstream_branch_name = None;
        let tag_names = HashMap::new();
        (pristine_revids, tag_names, upstream_branch_name)
    } else {
        let (pristine_revids, tag_names, upstream_branch_name) = import_upstream_version_from_dist(
            wt,
            subpath,
            upstream_source,
            source_name,
            upstream_version,
        )?;
        (pristine_revids, tag_names, Some(upstream_branch_name))
    };

    let orig_revid = pristine_revids.remove(&TarballKind::Orig).unwrap().0;
    Ok((orig_revid, upstream_branch_name, tag_names))
}

/// Generate an upstream version for a package if all else fails.
pub fn last_resort_upstream_version(
    upstream_source: &UpstreamBranchSource,
    upstream_revision: &RevisionId,
) -> Result<String, BrzDebianError> {
    let upstream_version = upstream_version_add_revision(
        upstream_source.upstream_branch().as_ref(),
        "0",
        upstream_revision,
        Some("+"),
    )?;
    log::warn!(
        "Unable to determine upstream version, using {}.",
        upstream_version
    );
    Ok(upstream_version)
}

#[derive(Debug, Clone)]
pub enum SessionPreferences {
    Plain,
    Schroot(String),
    Unshare(String),
}

pub struct DebianizePreferences {
    pub use_inotify: Option<bool>,
    pub diligence: u8,
    pub trust: bool,
    pub check: bool,
    pub net_access: bool,
    pub force_subprocess: bool,
    pub force_new_directory: bool,
    pub compat_release: Option<String>,
    pub minimum_certainty: Certainty,
    pub consult_external_directory: bool,
    pub verbose: bool,
    pub session: SessionPreferences,
    pub create_dist: Option<Box<dyn for <'a, 'b, 'c, 'd, 'e>Fn(&'a dyn Tree, &'b str, &'c Version, &'d Path, &'e Path) -> Result<bool, breezyshim::debian::error::Error>>>,
    pub committer: Option<String>,
    pub upstream_version_kind: VersionKind,
    pub debian_revision: String,
    pub team: Option<String>,
    pub author: Option<String>,
}

impl Default for DebianizePreferences {
    fn default() -> Self {
        let author = debian_changelog::get_maintainer();
        Self {
            use_inotify: None,
            diligence: 0,
            trust: false,
            check: false,
            net_access: true,
            force_subprocess: false,
            force_new_directory: false,
            compat_release: None,
            minimum_certainty: Certainty::Confident,
            consult_external_directory: true,
            verbose: false,
            session: SessionPreferences::Plain,
            create_dist: None,
            committer: None,
            upstream_version_kind: VersionKind::Auto,
            debian_revision: "1".to_string(),
            team: None,
            author: author.map(|(name, email)| format!("{} <{}>", name, email)),
        }
    }
}

impl From<DebianizePreferences> for lintian_brush::FixerPreferences {
    fn from(p: DebianizePreferences) -> Self {
        Self {
            diligence: Some(p.diligence.into()),
            net_access: Some(p.net_access),
            compat_release: p.compat_release,
            minimum_certainty: Some(p.minimum_certainty),
            trust_package: Some(p.trust),
            opinionated: Some(true),
            allow_reformatting: Some(true),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    DebianDirectoryExists(PathBuf),
    DebianizedPackageRequirementMismatch {
        dep: DebianDependency,
        binary_names: Vec<String>,
        version: Version,
        branch: Option<url::Url>,
    },
    EditorError(debian_analyzer::editor::EditorError),
    MissingUpstreamInfo(String),
    NoVcsLocation,
    NoUpstreamReleases(Option<String>),
    SourcePackageNameInvalid(String),
    SubdirectoryNotFound {
        subpath: PathBuf,
        version: Option<String>,
    },
    IoError(std::io::Error),
    BrzError(BrzError),
}

impl From<BrzError> for Error {
    fn from(e: BrzError) -> Self {
        Error::BrzError(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IoError(e)
    }
}

impl From<debian_analyzer::editor::EditorError> for Error {
    fn from(e: debian_analyzer::editor::EditorError) -> Self {
        match e {
            debian_analyzer::editor::EditorError::IoError(e) => Error::IoError(e),
            debian_analyzer::editor::EditorError::BrzError(e) => Error::BrzError(e),
            e => Error::EditorError(e),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use Error::*;
        match self {
            DebianDirectoryExists(path) => {
                write!(f, "Debian directory already exists at {}.", path.display())
            }
            DebianizedPackageRequirementMismatch {
                dep,
                binary_names,
                version,
                branch,
            } => {
                write!(
                f,
                "Debianized package {} (version: {}) from {} does not match requirements for {}.",
                binary_names.join(", "),
                version,
                branch.as_ref().map_or_else(|| "unknown branch".to_string(), |b| b.to_string()),
                dep.relation_string(),
            )
            }
            NoVcsLocation => {
                write!(f, "No VCS location found.")
            }
            NoUpstreamReleases(source_name) => {
                write!(
                    f,
                    "No upstream releases found for {}.",
                    source_name.as_deref().unwrap_or("unknown")
                )
            }
            SourcePackageNameInvalid(name) => write!(f, "Invalid source package name: {}.", name),
            SubdirectoryNotFound { subpath, version } => {
                write!(
                    f,
                    "Subdirectory {} not found in upstream source{}.",
                    subpath.display(),
                    version
                        .as_ref()
                        .map(|v| format!(" for version {}", v))
                        .unwrap_or_default()
                )
            }
            IoError(e) => write!(f, "I/O error: {}", e),
            BrzError(e) => write!(f, "Breezy error: {}", e),
            MissingUpstreamInfo(name) => write!(f, "Missing upstream information for {}.", name),
            EditorError(e) => write!(f, "Editor error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

pub fn debianize(
    wt: &WorkingTree,
    subpath: &Path,
    upstream_branch: Option<&dyn Branch>,
    upstream_subpath: Option<&Path>,
    preferences: &DebianizePreferences,
    version: Option<&str>,
    upstream_metadata: &UpstreamMetadata,
) -> Result<DebianizeResult, Error> {
    Ok(DebianizeResult::default())
}

#[derive(Default, serde::Serialize)]
pub struct DebianizeResult {
    pub vcs_url: Option<url::Url>,
    pub wnpp_bugs: Vec<(i64, BugKind)>,
    pub upstream_version: Option<String>,
    pub tag_names: HashMap<String, RevisionId>,
    pub upstream_branch_name: Option<String>,
}

pub(crate) struct ResetOnFailure<'a>(&'a WorkingTree, PathBuf);

impl<'a> ResetOnFailure<'a> {
    pub fn new(wt: &'a WorkingTree, subpath: &Path) -> Result<Self, BrzError> {
        breezyshim::workspace::check_clean_tree(wt, &wt.basis_tree().unwrap(), subpath)?;
        Ok(Self(wt, subpath.to_path_buf()))
    }
}

impl<'a> Drop for ResetOnFailure<'a> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            match breezyshim::workspace::reset_tree(self.0, None, Some(&self.1)) {
                Ok(_) => {}
                Err(e) => log::error!("Failed to reset tree: {:?}", e),
            }
        }
    }
}

fn generic_get_source_name(
    wt: &WorkingTree,
    subpath: &Path,
    metadata: &UpstreamMetadata,
) -> Option<String> {
    let mut source_name = if let Some(name) = metadata.name() {
        let mut source_name = names::upstream_name_to_debian_source_name(name);
        if !valid_debian_package_name(source_name.as_ref().unwrap()) {
            source_name = None;
        }
        source_name
    } else {
        None
    };

    if source_name.is_none() {
        source_name = names::upstream_name_to_debian_source_name(
            wt.abspath(subpath).unwrap().to_str().unwrap(),
        );
        if !valid_debian_package_name(source_name.as_ref().unwrap()) {
            source_name = None;
        }
    }
    source_name
}

fn import_metadata_from_path(
    tree: &WorkingTree,
    subpath: &Path,
    metadata: &mut UpstreamMetadata,
    preferences: &DebianizePreferences,
) -> Result<(), ProviderError> {
    let p = tree.abspath(subpath).unwrap();
    let metadata_items =
        guess_upstream_info(&p, Some(preferences.trust)).collect::<Result<Vec<_>, _>>()?;
    metadata.update(
        summarize_upstream_metadata(
            metadata_items.into_iter(),
            &p,
            Some(preferences.net_access),
            Some(preferences.consult_external_directory),
            Some(preferences.check),
        )?
        .into_iter(),
    );
    Ok(())
}
