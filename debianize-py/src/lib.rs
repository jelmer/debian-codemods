use debianize::BugKind;
use debversion::Version;
use pyo3::prelude::*;

#[pyfunction]
fn default_debianize_cache_dir() -> PyResult<std::path::PathBuf> {
    Ok(debianize::default_debianize_cache_dir()?)
}

#[pyfunction]
fn write_changelog_template(
    path: std::path::PathBuf,
    source_name: &str,
    version: Version,
    author: Option<(String, String)>,
    wnpp_bugs: Option<Vec<(BugKind, u32)>>,
) -> Result<(), std::io::Error> {
    debianize::write_changelog_template(path.as_path(), source_name, &version, author, wnpp_bugs)?;
    Ok(())
}

#[pymodule]
fn _debianize_rs(py: Python, m: &PyModule) -> PyResult<()> {
    pyo3_log::init();

    m.add_function(wrap_pyfunction!(default_debianize_cache_dir, m)?)?;
    m.add_function(wrap_pyfunction!(write_changelog_template, m)?)?;

    Ok(())
}
