use debian_analyzer::publish::Error as PublishError;
use pyo3::exceptions::{PyException, PyFileNotFoundError};
use pyo3::prelude::*;
use pyo3::types::PyTuple;
use pyo3::create_exception;

create_exception!(lintian_brush.publish, NoVcsLocation, PyException);
create_exception!(
    lintian_brush.publish,
    ConflictingVcsAlreadySpecified,
    PyException
);

#[pyclass]
struct ChangelogBehaviour {
    update_changelog: bool,
    explanation: String,
}

#[pymethods]
impl ChangelogBehaviour {
    #[new]
    fn new(update_changelog: bool, explanation: String) -> Self {
        Self {
            update_changelog,
            explanation,
        }
    }

    fn __richcmp__(&self, other: PyRef<Self>, op: pyo3::pyclass::CompareOp) -> PyResult<bool> {
        match op {
            pyo3::pyclass::CompareOp::Eq => Ok(self.update_changelog == other.update_changelog
                && self.explanation == other.explanation),
            pyo3::pyclass::CompareOp::Ne => Ok(self.update_changelog != other.update_changelog
                || self.explanation != other.explanation),
            _ => Err(pyo3::exceptions::PyNotImplementedError::new_err(
                "only == and != are supported",
            )),
        }
    }

    fn __str__(&self) -> String {
        self.explanation.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "ChangelogBehaviour(update_changelog={}, explanation={})",
            self.update_changelog, &self.explanation
        )
    }
}

#[pyfunction]
fn guess_update_changelog(
    tree: PyObject,
    path: std::path::PathBuf,
) -> pyo3::PyResult<Option<PyObject>> {
    let path = path.as_path();
    Python::with_gil(|py| {
        let tree = breezyshim::tree::WorkingTree(tree);
        Ok(
            debian_analyzer::detect_gbp_dch::guess_update_changelog(&tree, path, None).map(|cb| {
                ChangelogBehaviour {
                    update_changelog: cb.update_changelog,
                    explanation: cb.explanation,
                }
                .into_py(py)
            }),
        )
    })
}

#[pyfunction]
#[pyo3(signature = (wt, subpath, repo_url=None, branch=None, committer=None, force=None))]
fn update_official_vcs(
    wt: PyObject,
    subpath: std::path::PathBuf,
    repo_url: Option<&str>,
    branch: Option<&str>,
    committer: Option<&str>,
    force: Option<bool>,
) -> PyResult<(String, Option<String>, Option<std::path::PathBuf>)> {
    let wt = breezyshim::WorkingTree(wt);

    let repo_url = repo_url.map(|s| s.parse().unwrap());

    match debian_analyzer::publish::update_official_vcs(
        &wt,
        subpath.as_path(),
        repo_url.as_ref(),
        branch,
        committer,
        force,
    ) {
        Ok(parsed_vcs) => Ok((
            parsed_vcs.repo_url,
            parsed_vcs.branch,
            parsed_vcs.subpath.map(Into::into),
        )),
        Err(PublishError::FileNotFound(p)) => Err(PyFileNotFoundError::new_err((p,))),
        Err(PublishError::NoVcsLocation) => Err(NoVcsLocation::new_err(())),
        Err(PublishError::ConflictingVcsAlreadySpecified(
            vcs_type,
            existing_vcs_url,
            target_vcs_url,
        )) => Err(ConflictingVcsAlreadySpecified::new_err((
            vcs_type,
            existing_vcs_url.to_string(),
            target_vcs_url.to_string(),
        ))),
    }
}

#[pyfunction]
fn guess_repository_url(package: &str, maintainer_email: &str) -> Option<String> {
    debian_analyzer::salsa::guess_repository_url(package, maintainer_email).map(|u| u.to_string())
}

#[pyfunction]
#[pyo3(signature = (vcs_type, vcs_url, net_access=None))]
fn determine_browser_url(
    vcs_type: &str,
    vcs_url: &str,
    net_access: Option<bool>,
) -> PyResult<Option<String>> {
    Ok(
        debian_analyzer::vcs::determine_browser_url(vcs_type, vcs_url, net_access)
            .map(|u| u.to_string()),
    )
}

#[pyfunction]
fn determine_gitlab_browser_url(url: &str) -> String {
    debian_analyzer::vcs::determine_gitlab_browser_url(url).to_string()
}

#[pyfunction]
fn canonicalize_vcs_browser_url(url: &str) -> String {
    debian_analyzer::vcs::canonicalize_vcs_browser_url(url).to_string()
}

#[pyfunction]
#[pyo3(signature = (tree, subpath=None))]
fn tree_patches_directory(
    tree: PyObject,
    subpath: Option<std::path::PathBuf>,
) -> std::path::PathBuf {
    let tree = breezyshim::tree::RevisionTree(tree);
    debian_analyzer::patches::tree_patches_directory(&tree, subpath.unwrap_or_default().as_path())
}

#[pyfunction]
fn find_patches_directory(
    tree: PyObject,
    subpath: std::path::PathBuf,
) -> Option<std::path::PathBuf> {
    let tree = breezyshim::tree::RevisionTree(tree);
    debian_analyzer::patches::find_patches_directory(&tree, subpath.as_path())
}

#[pyfunction]
#[pyo3(signature = (tree, patches_directory=None))]
fn tree_has_non_patches_changes(
    tree: PyObject,
    patches_directory: Option<std::path::PathBuf>,
) -> PyResult<bool> {
    let tree = breezyshim::workingtree::WorkingTree(tree);
    Ok(
        !debian_analyzer::patches::tree_non_patches_changes(tree, patches_directory.as_deref())?
            .is_empty(),
    )
}

#[pymodule]
fn _lintian_brush_rs(py: Python, m: &Bound<PyModule>) -> PyResult<()> {
    pyo3_log::init();
    let v = PyTuple::new_bound(
        py,
        env!("CARGO_PKG_VERSION")
            .split('.')
            .map(|x| x.parse::<u32>().unwrap())
            .collect::<Vec<u32>>(),
    );
    m.add("__version__", v)?;
    m.add_class::<ChangelogBehaviour>()?;
    m.add_wrapped(wrap_pyfunction!(tree_has_non_patches_changes))?;
    m.add_wrapped(wrap_pyfunction!(guess_update_changelog))?;
    m.add_wrapped(wrap_pyfunction!(update_official_vcs))?;
    m.add_wrapped(wrap_pyfunction!(guess_repository_url))?;
    m.add_wrapped(wrap_pyfunction!(determine_browser_url))?;
    m.add_wrapped(wrap_pyfunction!(determine_gitlab_browser_url))?;
    m.add_wrapped(wrap_pyfunction!(canonicalize_vcs_browser_url))?;
    m.add_wrapped(wrap_pyfunction!(tree_patches_directory))?;
    m.add_wrapped(wrap_pyfunction!(find_patches_directory))?;
    m.add("NoVcsLocation", py.get_type_bound::<NoVcsLocation>())?;
    m.add(
        "ConflictingVcsAlreadySpecified",
        py.get_type_bound::<ConflictingVcsAlreadySpecified>(),
    )?;
    Ok(())
}
