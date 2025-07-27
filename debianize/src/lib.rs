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
use breezyshim::tree::{MutableTree, PyTree};
use breezyshim::workingtree::{GenericWorkingTree, WorkingTree};
use breezyshim::RevisionId;
use debian_analyzer::versions::debianize_upstream_version;
use debian_analyzer::wnpp::BugKind;
use debian_analyzer::Certainty;
use debversion::Version;
use ognibuild::dependencies::debian::valid_debian_package_name;
use ognibuild::dependencies::debian::DebianDependency;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use upstream_ontologist::{get_upstream_info, ProviderError, UpstreamMetadata};

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

pub fn use_packaging_branch(wt: &GenericWorkingTree, branch_name: &str) -> Result<(), BrzError> {
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
        use pyo3::IntoPyObject;
        let wt_py = wt.to_object(py);
        let branch_py = target_branch.into_pyobject(py)?;
        wt_py.setattr(py, "_branch", branch_py)?;
        Ok(())
    })
    .unwrap();
    Ok(())
}

pub fn import_upstream_version_from_dist(
    wt: &GenericWorkingTree,
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
    let files_excluded: Option<&[&std::path::Path]> = None;
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
    wt: &GenericWorkingTree,
    upstream_source: &UpstreamBranchSource,
    subpath: &Path,
    source_name: &str,
    upstream_version: &UpstreamVersion,
) -> Result<(RevisionId, Option<String>, HashMap<TarballKind, String>), BrzDebianError> {
    let (mut pristine_revids, tag_names, upstream_branch_name) = if pristine_tar_source
        .has_version(Some(source_name), &upstream_version.version, None, false)?
    {
        log::warn!(
            "Upstream version {}/{} already imported.",
            source_name,
            upstream_version.version,
        );
        let pristine_revids = pristine_tar_source.version_as_revisions(
            Some(source_name),
            &upstream_version.version,
            None,
        )?;
        let upstream_branch_name = None;
        let tag_names = HashMap::new();
        (pristine_revids, tag_names, upstream_branch_name)
    } else {
        let (pristine_revids, tag_names, upstream_branch_name) = import_upstream_version_from_dist(
            wt,
            subpath,
            upstream_source,
            source_name,
            &upstream_version.version,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_write_changelog_template() {
        let td = tempdir().unwrap();
        let path = td.path().join("changelog");

        let source_name = "test-package";
        let version = Version {
            epoch: None,
            upstream_version: "1.0".to_string(),
            debian_revision: Some("1".to_string()),
        };
        let author = Some(("Test Author".to_string(), "test@example.com".to_string()));
        let wnpp_bugs = vec![(123456, BugKind::ITP)];

        write_changelog_template(&path, source_name, &version, author, wnpp_bugs).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("test-package (1.0-1) UNRELEASED"));
        assert!(content.contains("* Initial release. Closes: #123456"));
        assert!(content.contains("Test Author <test@example.com>"));
    }

    #[test]
    fn test_write_changelog_template_no_bugs() {
        let td = tempdir().unwrap();
        let path = td.path().join("changelog");

        let source_name = "test-package";
        let version = Version {
            epoch: None,
            upstream_version: "1.0".to_string(),
            debian_revision: Some("1".to_string()),
        };
        let author = Some(("Test Author".to_string(), "test@example.com".to_string()));
        let wnpp_bugs = vec![];

        write_changelog_template(&path, source_name, &version, author, wnpp_bugs).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("test-package (1.0-1) UNRELEASED"));
        assert!(content.contains("* Initial release."));
        assert!(!content.contains("Closes:"));
        assert!(content.contains("Test Author <test@example.com>"));
    }

    #[test]
    fn test_default_debianize_cache_dir() {
        // This test is a bit tricky to verify as it accesses the real XDG dirs
        // We'll just check that it returns a result (doesn't error out)
        let result = default_debianize_cache_dir();
        assert!(result.is_ok());
    }

    // We'll skip testing the debianize stub function since it requires a WorkingTree
    // which is difficult to create in a test environment without actual repository data

    #[test]
    fn test_upstream_version_from() {
        // Test the From<String> implementation for UpstreamVersion
        let version_str = "1.2.3".to_string();
        let upstream_version = UpstreamVersion::from(version_str.clone());

        assert_eq!(upstream_version.version, "1.2.3");
        assert_eq!(
            upstream_version.mangled_version,
            debianize_upstream_version(&version_str)
        );
    }

    #[test]
    fn test_debianize_preferences_default() {
        // Test the default implementation of DebianizePreferences
        let prefs = DebianizePreferences::default();

        assert_eq!(prefs.use_inotify, None);
        assert_eq!(prefs.diligence, 0);
        assert_eq!(prefs.trust, false);
        assert_eq!(prefs.check, false);
        assert_eq!(prefs.net_access, true);
        assert_eq!(prefs.force_subprocess, false);
        assert_eq!(prefs.force_new_directory, false);
        assert_eq!(prefs.compat_release, None);
        assert_eq!(prefs.minimum_certainty, Certainty::Confident);
        assert_eq!(prefs.consult_external_directory, true);
        assert_eq!(prefs.verbose, false);
        match prefs.session {
            SessionPreferences::Plain => {}
            _ => panic!("Expected SessionPreferences::Plain"),
        }
        assert!(prefs.create_dist.is_none());
        assert!(prefs.committer.is_none());
        assert_eq!(prefs.upstream_version_kind, VersionKind::Auto);
        assert_eq!(prefs.debian_revision, "1".to_string());
        assert!(prefs.team.is_none());
    }

    #[test]
    fn test_debianize_preferences_into_fixer_preferences() {
        // Test the conversion from DebianizePreferences to lintian_brush::FixerPreferences
        let debianize_prefs = DebianizePreferences {
            use_inotify: Some(true),
            diligence: 2,
            trust: true,
            check: true,
            net_access: false,
            force_subprocess: true,
            force_new_directory: true,
            compat_release: Some("stable".to_string()),
            minimum_certainty: Certainty::Certain,
            consult_external_directory: false,
            verbose: true,
            session: SessionPreferences::Plain,
            create_dist: None,
            committer: None,
            upstream_version_kind: VersionKind::Release,
            debian_revision: "2".to_string(),
            team: None,
            author: None,
        };

        let fixer_prefs: lintian_brush::FixerPreferences = debianize_prefs.into();

        assert_eq!(fixer_prefs.diligence, Some(2));
        assert_eq!(fixer_prefs.net_access, Some(false));
        assert_eq!(fixer_prefs.compat_release, Some("stable".to_string()));
        assert_eq!(fixer_prefs.minimum_certainty, Some(Certainty::Certain));
        assert_eq!(fixer_prefs.trust_package, Some(true));
        assert_eq!(fixer_prefs.opinionated, Some(true));
        assert_eq!(fixer_prefs.allow_reformatting, Some(true));
    }

    #[test]
    fn test_error_display() {
        // Test the Display implementation for Error
        let error = Error::NoVcsLocation;
        assert_eq!(format!("{}", error), "No VCS location found.");

        let error = Error::SourcePackageNameInvalid("invalid:name".to_string());
        assert_eq!(
            format!("{}", error),
            "Invalid source package name: invalid:name."
        );

        let error = Error::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "File not found",
        ));
        assert_eq!(format!("{}", error), "I/O error: File not found");

        let error = Error::NoUpstreamReleases(Some("test-package".to_string()));
        assert_eq!(
            format!("{}", error),
            "No upstream releases found for test-package."
        );

        let error = Error::NoUpstreamReleases(None);
        assert_eq!(
            format!("{}", error),
            "No upstream releases found for unknown."
        );
    }
}

#[derive(Debug, Clone)]
pub enum SessionPreferences {
    Plain,
    Schroot(String),
    Unshare(PathBuf),
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
    pub create_dist: Option<
        Box<
            dyn for<'a, 'b, 'c, 'd, 'e> Fn(
                &'a dyn PyTree,
                &'b str,
                &'c Version,
                &'d Path,
                &'e Path,
            )
                -> Result<bool, breezyshim::debian::error::Error>,
        >,
    >,
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
            upgrade_release: None,
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
    SqlxError(sqlx::Error),
}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Error::SqlxError(e)
    }
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
            SqlxError(e) => write!(f, "SQLx error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

pub fn debianize(
    _wt: &dyn WorkingTree,
    _subpath: &Path,
    _upstream_branch: Option<&dyn Branch>,
    _upstream_subpath: Option<&Path>,
    _preferences: &DebianizePreferences,
    _version: Option<&str>,
    _upstream_metadata: &UpstreamMetadata,
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

pub(crate) struct ResetOnFailure<'a>(&'a GenericWorkingTree, PathBuf);

impl<'a> ResetOnFailure<'a> {
    pub fn new(wt: &'a GenericWorkingTree, subpath: &Path) -> Result<Self, BrzError> {
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
    wt: &dyn WorkingTree,
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
    tree: &dyn WorkingTree,
    subpath: &Path,
    metadata: &mut UpstreamMetadata,
    preferences: &DebianizePreferences,
) -> Result<(), ProviderError> {
    let p = tree.abspath(subpath).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    metadata.update(rt.block_on(get_upstream_info(
        &p,
        Some(preferences.trust),
        Some(preferences.net_access),
        Some(preferences.consult_external_directory),
        Some(preferences.check),
    ))?);
    Ok(())
}

pub struct UpstreamVersion {
    pub version: String,
    pub mangled_version: String,
}

impl From<String> for UpstreamVersion {
    fn from(v: String) -> Self {
        Self {
            version: v.clone(),
            mangled_version: debianize_upstream_version(&v),
        }
    }
}

/// Determine the upstream version to use.
pub fn determine_upstream_version(
    upstream_source: &UpstreamBranchSource,
    metadata: &UpstreamMetadata,
    version_kind: VersionKind,
) -> Result<UpstreamVersion, Error> {
    let name = metadata.name();

    // Ask the upstream source for the latest version.
    if let Some((upstream_version, mangled_version)) =
        upstream_source.get_latest_version(name, None).unwrap()
    {
        return Ok(UpstreamVersion {
            version: upstream_version,
            mangled_version,
        });
    }

    if version_kind == VersionKind::Release {
        return Err(Error::NoUpstreamReleases(
            metadata.name().map(|x| x.to_string()),
        ));
    }

    let upstream_revision = upstream_source.upstream_branch().last_revision();

    if let Some(next_upstream_version) = metadata.version() {
        // They haven't done any releases yet. Assume we're ahead of the next announced release?
        let next_upstream_version = debianize_upstream_version(next_upstream_version);
        let upstream_version = upstream_version_add_revision(
            upstream_source.upstream_branch().as_ref(),
            &next_upstream_version,
            &upstream_revision,
            Some("~"),
        )
        .map_err(|e| match e {
            BrzDebianError::BrzError(brz_err) => Error::BrzError(brz_err),
            _ => Error::BrzError(BrzError::Other(pyo3::PyErr::new::<
                pyo3::exceptions::PyRuntimeError,
                _,
            >(format!(
                "Debian error: {:?}",
                e
            )))),
        })?;
        return Ok(UpstreamVersion::from(upstream_version));
    }

    let upstream_version = upstream_version_add_revision(
        upstream_source.upstream_branch().as_ref(),
        "0",
        &upstream_revision,
        Some("+"),
    )
    .map_err(|e| match e {
        BrzDebianError::BrzError(brz_err) => Error::BrzError(brz_err),
        _ => Error::BrzError(BrzError::Other(pyo3::PyErr::new::<
            pyo3::exceptions::PyRuntimeError,
            _,
        >(format!("Debian error: {:?}", e)))),
    })?;
    log::warn!(
        "Unable to determine upstream version, using {}.",
        upstream_version
    );
    Ok(UpstreamVersion::from(upstream_version))
}
