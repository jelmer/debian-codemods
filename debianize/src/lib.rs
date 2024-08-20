use breezyshim::debian::error::Error as BrzDebianError;
use breezyshim::debian::merge_upstream::{
    do_import, get_existing_imported_upstream_revids, get_tarballs,
};
use breezyshim::debian::upstream::{PristineTarSource, UpstreamBranchSource, UpstreamSource};
use breezyshim::debian::{TarballKind, DEFAULT_ORIG_DIR};
use breezyshim::error::Error as BrzError;
use breezyshim::workingtree::WorkingTree;
use breezyshim::RevisionId;
use debversion::Version;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub mod simple_apt_repo;

pub fn default_debianize_cache_dir() -> std::io::Result<std::path::PathBuf> {
    xdg::BaseDirectories::with_prefix("debianize")?.create_cache_directory("")
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BugKind {
    RFP,
    ITP,
}

impl std::str::FromStr for BugKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "RFP" => Ok(BugKind::RFP),
            "ITP" => Ok(BugKind::ITP),
            _ => Err(format!("Unknown bug kind: {}", s)),
        }
    }
}

impl std::fmt::Display for BugKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BugKind::RFP => write!(f, "RFP"),
            BugKind::ITP => write!(f, "ITP"),
        }
    }
}

#[cfg(feature = "pyo3")]
impl pyo3::FromPyObject<'_> for BugKind {
    fn extract_bound(ob: &pyo3::Bound<pyo3::PyAny>) -> pyo3::PyResult<Self> {
        use pyo3::prelude::*;
        let s: String = ob.extract()?;
        s.parse().map_err(pyo3::exceptions::PyValueError::new_err)
    }
}

pub fn write_changelog_template(
    path: &std::path::Path,
    source_name: &str,
    version: &Version,
    author: Option<(String, String)>,
    wnpp_bugs: Option<Vec<(BugKind, u32)>>,
) -> Result<(), std::io::Error> {
    let author = author.unwrap_or_else(|| debian_changelog::get_maintainer().unwrap());
    let closes = if let Some(wnpp_bugs) = wnpp_bugs {
        format!(
            " Closes: {}",
            wnpp_bugs
                .iter()
                .map(|(_k, n)| format!("#{}", n))
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

pub fn source_name_from_directory_name(path: &std::path::Path) -> String {
    let d = path.file_name().unwrap().to_str().unwrap();
    if d.contains('-') {
        let mut parts = d.split('-').collect::<Vec<_>>();
        let c = parts.last().unwrap().chars().next().unwrap();
        if c.is_ascii_digit() {
            parts.pop();
            return parts.join("-");
        }
    }
    d.to_string()
}

pub fn go_import_path_from_repo(repo_url: &url::Url) -> String {
    repo_url.host_str().unwrap().to_string()
        + repo_url
            .path()
            .trim_end_matches('/')
            .trim_end_matches(".git")
}

pub fn perl_package_name(upstream_name: &str) -> String {
    let upstream_name = upstream_name.strip_prefix("lib").unwrap_or(upstream_name);
    format!(
        "lib{}-perl",
        upstream_name
            .replace("::", "-")
            .replace('_', "")
            .to_lowercase()
    )
}

pub fn python_source_package_name(upstream_name: &str) -> String {
    let upstream_name = upstream_name
        .strip_prefix("python-")
        .unwrap_or(upstream_name);
    format!("python-{}", upstream_name.replace('_', "-").to_lowercase())
}

pub fn python_binary_package_name(upstream_name: &str) -> String {
    let upstream_name = upstream_name
        .strip_prefix("python-")
        .unwrap_or(upstream_name);
    format!("python3-{}", upstream_name.replace('_', "-").to_lowercase())
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
        source_name,
        upstream_version,
        td.path(),
        Some(&[TarballKind::Orig]),
    )?;
    let tarball_filenames = match get_tarballs(
        &orig_dir,
        wt,
        &source_name,
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
        upstream_source.version_as_revisions(&source_name, upstream_version, None)?;
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
        .has_version(source_name, upstream_version, None, false)?
    {
        log::warn!(
            "Upstream version {}/{} already imported.",
            source_name,
            upstream_version,
        );
        let pristine_revids =
            pristine_tar_source.version_as_revisions(source_name, upstream_version, None)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_name_from_directory_name() {
        assert_eq!(
            source_name_from_directory_name(std::path::Path::new("foo")),
            "foo"
        );
        assert_eq!(
            source_name_from_directory_name(std::path::Path::new("foo-bar")),
            "foo-bar"
        );
        assert_eq!(
            source_name_from_directory_name(std::path::Path::new("foo-bar-1")),
            "foo-bar"
        );
        assert_eq!(
            source_name_from_directory_name(std::path::Path::new("foo-bar-1.0")),
            "foo-bar"
        );
    }

    #[test]
    fn test_go_import_path_from_repo() {
        assert_eq!(
            go_import_path_from_repo(&url::Url::parse("https://github.com/foo/bar.git").unwrap()),
            "github.com/foo/bar"
        );
    }

    #[test]
    fn test_perl_package_name() {
        assert_eq!(perl_package_name("Foo::Bar"), "libfoo-bar-perl");
        assert_eq!(perl_package_name("Foo::Bar::Baz"), "libfoo-bar-baz-perl");
        assert_eq!(
            perl_package_name("Foo::Bar::Baz::Qux"),
            "libfoo-bar-baz-qux-perl"
        );
        assert_eq!(
            perl_package_name("Foo::Bar::Baz::Qux::Quux"),
            "libfoo-bar-baz-qux-quux-perl"
        );
        assert_eq!(
            perl_package_name("Foo::Bar::Baz::Qux::Quux::Corge"),
            "libfoo-bar-baz-qux-quux-corge-perl"
        );
    }

    #[test]
    fn test_python_source_package_name() {
        assert_eq!(python_source_package_name("foo"), "python-foo");
        assert_eq!(
            python_source_package_name("python-foo_bar"),
            "python-foo-bar"
        );
        assert_eq!(python_source_package_name("foo_bar"), "python-foo-bar");
        assert_eq!(
            python_source_package_name("foo_bar_baz"),
            "python-foo-bar-baz"
        );
    }

    #[test]
    fn test_python_binary_package_name() {
        assert_eq!(python_binary_package_name("foo"), "python3-foo");
        assert_eq!(
            python_binary_package_name("python-foo_bar"),
            "python3-foo-bar"
        );
        assert_eq!(python_binary_package_name("foo_bar"), "python3-foo-bar");
        assert_eq!(
            python_binary_package_name("foo_bar_baz"),
            "python3-foo-bar-baz"
        );
    }
}
