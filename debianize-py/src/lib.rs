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

#[pyfunction]
fn source_name_from_directory_name(path: std::path::PathBuf) -> String {
    debianize::source_name_from_directory_name(path.as_path())
}

#[pymodule]
fn _debianize_rs(_py: Python, m: &Bound<PyModule>) -> PyResult<()> {
    pyo3_log::init();

    m.add_function(wrap_pyfunction!(default_debianize_cache_dir, m)?)?;
    m.add_function(wrap_pyfunction!(write_changelog_template, m)?)?;
    m.add_function(wrap_pyfunction!(source_name_from_directory_name, m)?)?;

    Ok(())
}
