use pyo3::prelude::*;
use pyo3::types::PyTuple;

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
    m.add_wrapped(wrap_pyfunction!(tree_has_non_patches_changes))?;
    m.add_wrapped(wrap_pyfunction!(guess_repository_url))?;
    m.add_wrapped(wrap_pyfunction!(determine_browser_url))?;
    m.add_wrapped(wrap_pyfunction!(determine_gitlab_browser_url))?;
    m.add_wrapped(wrap_pyfunction!(canonicalize_vcs_browser_url))?;
    m.add_wrapped(wrap_pyfunction!(tree_patches_directory))?;
    m.add_wrapped(wrap_pyfunction!(find_patches_directory))?;
    Ok(())
}
