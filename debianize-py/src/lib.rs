use debianize::BugKind;
use debversion::Version;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

#[pyfunction]
fn default_debianize_cache_dir() -> PyResult<std::path::PathBuf> {
    Ok(debianize::default_debianize_cache_dir()?)
}

#[pyfunction]
fn go_import_path_from_repo(url: &str) -> PyResult<String> {
    let url: url::Url = url
        .parse()
        .map_err(|e: url::ParseError| PyValueError::new_err((e.to_string(),)))?;
    Ok(debianize::names::go_import_path_from_repo(&url))
}

#[pyfunction]
fn perl_package_name(name: &str) -> String {
    debianize::names::perl_package_name(name)
}

#[pyfunction]
#[pyo3(signature = (path, source_name, version, author=None, wnpp_bugs=None))]
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
    debianize::names::source_name_from_directory_name(path.as_path())
}

#[pyfunction]
fn python_source_package_name(name: &str) -> String {
    debianize::names::python_source_package_name(name)
}

#[pyfunction]
fn python_binary_package_name(name: &str) -> String {
    debianize::names::python_binary_package_name(name)
}

#[pymodule]
fn _debianize_rs(_py: Python, m: &Bound<PyModule>) -> PyResult<()> {
    pyo3_log::init();

    m.add_function(wrap_pyfunction!(default_debianize_cache_dir, m)?)?;
    m.add_function(wrap_pyfunction!(write_changelog_template, m)?)?;
    m.add_function(wrap_pyfunction!(source_name_from_directory_name, m)?)?;
    m.add_function(wrap_pyfunction!(go_import_path_from_repo, m)?)?;
    m.add_function(wrap_pyfunction!(perl_package_name, m)?)?;
    m.add_function(wrap_pyfunction!(python_source_package_name, m)?)?;
    m.add_function(wrap_pyfunction!(python_binary_package_name, m)?)?;

    Ok(())
}
