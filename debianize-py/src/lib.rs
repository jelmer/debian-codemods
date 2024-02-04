use pyo3::prelude::*;

#[pyfunction]
fn default_debianize_cache_dir() -> PyResult<String> {
    Ok(debianize::debfault_debianize_cache_dir()?)
}

#[pymodule]
fn _debianize_rs(py: Python, m: &PyModule) -> PyResult<()> {
    pyo3_log::init();

    m.add_function(wrap_pyfunction!(debfault_debianize_cache_dir, m)?)?;

    Ok(())
}
