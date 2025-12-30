use pyo3::prelude::*;
use pyo3::types::PyTuple;

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
fn get_builtin_fixer_lintian_tags() -> Vec<String> {
    lintian_brush::builtin_fixers::get_builtin_fixers()
        .iter()
        .flat_map(|fixer| fixer.lintian_tags())
        .collect()
}

#[pyfunction]
#[pyo3(signature = (tree, subpath=None))]
fn tree_patches_directory(
    tree: Py<PyAny>,
    subpath: Option<std::path::PathBuf>,
) -> std::path::PathBuf {
    let tree = breezyshim::tree::RevisionTree(tree);
    debian_analyzer::patches::tree_patches_directory(&tree, subpath.unwrap_or_default().as_path())
}

#[pyfunction]
fn find_patches_directory(
    tree: Py<PyAny>,
    subpath: std::path::PathBuf,
) -> Option<std::path::PathBuf> {
    let tree = breezyshim::tree::RevisionTree(tree);
    debian_analyzer::patches::find_patches_directory(&tree, subpath.as_path())
}

#[pymodule]
fn _lintian_brush_rs(py: Python, m: &Bound<PyModule>) -> PyResult<()> {
    pyo3_log::init();
    let version_parts: Vec<u32> = env!("CARGO_PKG_VERSION")
        .split('.')
        .map(|x| x.parse::<u32>().unwrap())
        .collect();
    let v = PyTuple::new(py, &version_parts)?;
    m.add("__version__", &v)?;
    m.add_wrapped(wrap_pyfunction!(determine_browser_url))?;
    m.add_wrapped(wrap_pyfunction!(determine_gitlab_browser_url))?;
    m.add_wrapped(wrap_pyfunction!(canonicalize_vcs_browser_url))?;
    m.add_wrapped(wrap_pyfunction!(tree_patches_directory))?;
    m.add_wrapped(wrap_pyfunction!(find_patches_directory))?;
    m.add(
        "DPKG_VERSIONS",
        debian_analyzer::release_info::dpkg_versions.clone(),
    )?;
    m.add(
        "DEBHELPER_VERSIONS",
        debian_analyzer::release_info::debhelper_versions.clone(),
    )?;
    m.add_wrapped(wrap_pyfunction!(get_builtin_fixer_lintian_tags))?;
    Ok(())
}
