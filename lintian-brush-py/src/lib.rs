use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyType};

use pyo3::import_exception;

use std::collections::HashMap;

use lintian_brush::py::{
    json_to_py, py_to_json, Fixer, FixerResult, LintianIssue, PythonScriptFixer, ScriptFixer,
    UnsupportedCertainty,
};
use lintian_brush::Certainty;

import_exception!(lintian_brush, NoChanges);
import_exception!(lintian_brush, FixerScriptFailed);

#[pyfunction]
fn parse_script_fixer_output(text: &str) -> PyResult<FixerResult> {
    let result = lintian_brush::parse_script_fixer_output(text).map_err(|e| match e {
        lintian_brush::OutputParseError::LintianIssueParseError(e) => {
            PyValueError::new_err(format!("invalid lintian issue: {}", e))
        }
        lintian_brush::OutputParseError::UnsupportedCertainty(e) => {
            UnsupportedCertainty::new_err(e)
        }
    })?;
    Ok(FixerResult(result))
}

#[pyfunction]
pub fn determine_env(
    package: &str,
    current_version: &str,
    compat_release: &str,
    minimum_certainty: &str,
    trust_package: bool,
    allow_reformatting: bool,
    net_access: bool,
    opinionated: bool,
    diligence: i32,
) -> PyResult<std::collections::HashMap<String, String>> {
    let minimum_certainty = minimum_certainty
        .parse()
        .map_err(|e| UnsupportedCertainty::new_err(e))?;

    Ok(lintian_brush::determine_env(
        package,
        current_version,
        compat_release,
        minimum_certainty,
        trust_package,
        allow_reformatting,
        net_access,
        opinionated,
        diligence,
    ))
}

#[pyfunction]
fn read_desc_file(
    path: std::path::PathBuf,
    force_subprocess: Option<bool>,
) -> PyResult<Vec<Fixer>> {
    let force_subprocess = force_subprocess.unwrap_or(false);
    Ok(lintian_brush::read_desc_file(path, force_subprocess)
        .map_err(|e| PyValueError::new_err(e.to_string()))?
        .map(|s| Fixer(s))
        .collect())
}

#[pyfunction]
fn available_lintian_fixers(
    fixers_dir: std::path::PathBuf,
    force_subprocess: Option<bool>,
) -> PyResult<Vec<Fixer>> {
    Ok(
        lintian_brush::available_lintian_fixers(Some(fixers_dir.as_path()), force_subprocess)
            .map_err(|e| PyValueError::new_err(e.to_string()))?
            .map(|s| Fixer(s))
            .collect(),
    )
}

#[pyfunction]
fn certainty_sufficient(
    actual_certainty: Option<&str>,
    minimum_certainty: Option<&str>,
) -> PyResult<bool> {
    let actual_certainty = if let Some(actual_certainty) = actual_certainty {
        actual_certainty
            .parse()
            .map_err(UnsupportedCertainty::new_err)?
    } else {
        return Ok(true);
    };
    let minimum_certainty = minimum_certainty
        .map(|c| c.parse().map_err(UnsupportedCertainty::new_err))
        .transpose()?;
    Ok(lintian_brush::certainty_sufficient(
        actual_certainty,
        minimum_certainty,
    ))
}

#[pyfunction]
fn min_certainty(certainties: Vec<&str>) -> PyResult<String> {
    let certainties = certainties
        .iter()
        .map(|c| c.parse().map_err(UnsupportedCertainty::new_err))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lintian_brush::min_certainty(certainties.as_slice())
        .unwrap_or(Certainty::Certain)
        .to_string())
}

#[pyfunction]
fn resolve_release_codename(name: &str, date: Option<chrono::NaiveDate>) -> Option<String> {
    lintian_brush::release_info::resolve_release_codename(name, date)
}

#[pyfunction]
fn calculate_value(tags: Vec<&str>) -> i32 {
    lintian_brush::calculate_value(tags.as_slice())
}

#[pyfunction(name = "calculate_value")]
fn calculate_multiarch_value(hints: Vec<&str>) -> i32 {
    multiarch_hints::calculate_value(hints.as_slice())
}

#[pyfunction]
fn cache_download_multiarch_hints(py: Python, url: Option<&str>) -> PyResult<PyObject> {
    multiarch_hints::cache_download_multiarch_hints(url)
        .map_err(|e| PyValueError::new_err(e.to_string()))
        .map(|b| PyBytes::new(py, b.as_slice()).to_object(py))
}

#[pyfunction]
fn download_multiarch_hints(py: Python, url: Option<&str>) -> PyResult<PyObject> {
    multiarch_hints::download_multiarch_hints(url, None)
        .map_err(|e| PyValueError::new_err(e.to_string()))
        .map(|b| PyBytes::new(py, b.unwrap().as_slice()).to_object(py))
}

#[pyfunction]
fn report_fatal(
    versions: HashMap<String, String>,
    code: &str,
    description: &str,
    hint: Option<&str>,
) {
    lintian_brush::svp::report_fatal(versions, code, description, hint)
}

#[pyfunction]
pub fn report_success(
    py: Python,
    versions: HashMap<String, String>,
    value: Option<i32>,
    context: Option<PyObject>,
) -> PyResult<()> {
    let context = if let Some(context) = context {
        Some(py_to_json(py, context)?)
    } else {
        None
    };

    lintian_brush::svp::report_success(versions, value, context);
    Ok(())
}

#[pyfunction]
pub fn report_success_debian(
    py: Python,
    versions: HashMap<String, String>,
    value: Option<i32>,
    context: Option<PyObject>,
    changelog: Option<(bool, String)>,
) -> PyResult<()> {
    let context = if let Some(context) = context {
        Some(py_to_json(py, context)?)
    } else {
        None
    };
    lintian_brush::svp::report_success_debian(versions, value, context, changelog);
    Ok(())
}

#[pyclass]
struct Config(lintian_brush::config::Config);

#[pymethods]
impl Config {
    #[new]
    fn new(path: std::path::PathBuf) -> PyResult<Self> {
        Ok(Config(lintian_brush::config::Config::load_from_path(
            path.as_path(),
        )?))
    }

    #[classmethod]
    fn from_workingtree(_cls: &PyType, py: Python, wt: PyObject, subpath: &str) -> PyResult<Self> {
        let basedir = wt
            .getattr(py, "basedir")?
            .extract::<std::path::PathBuf>(py)?;
        let path = basedir
            .join(subpath)
            .join(lintian_brush::config::PACKAGE_CONFIG_FILENAME);
        Config::new(path)
    }

    pub fn compat_release(&self) -> Option<String> {
        self.0.compat_release()
    }

    pub fn allow_reformatting(&self) -> Option<bool> {
        self.0.allow_reformatting()
    }

    pub fn minimum_certainty(&self) -> Option<String> {
        self.0.minimum_certainty().map(|c| c.to_string())
    }

    pub fn update_changelog(&self) -> Option<bool> {
        self.0.update_changelog()
    }
}

#[pymodule]
fn _lintian_brush_rs(py: Python, m: &PyModule) -> PyResult<()> {
    pyo3_log::init();
    m.add_class::<LintianIssue>()?;
    m.add_class::<FixerResult>()?;
    m.add_wrapped(wrap_pyfunction!(parse_script_fixer_output))?;
    m.add(
        "UnsupportedCertainty",
        py.get_type::<UnsupportedCertainty>(),
    )?;
    m.add_wrapped(wrap_pyfunction!(determine_env))?;
    m.add_class::<Fixer>()?;
    m.add_class::<ScriptFixer>()?;
    m.add_class::<PythonScriptFixer>()?;
    m.add_wrapped(wrap_pyfunction!(read_desc_file))?;
    m.add_wrapped(wrap_pyfunction!(available_lintian_fixers))?;
    m.add_wrapped(wrap_pyfunction!(certainty_sufficient))?;
    m.add_wrapped(wrap_pyfunction!(min_certainty))?;
    m.add_wrapped(wrap_pyfunction!(resolve_release_codename))?;
    m.add_wrapped(wrap_pyfunction!(calculate_value))?;
    m.add(
        "DEFAULT_VALUE_LINTIAN_BRUSH",
        lintian_brush::DEFAULT_VALUE_LINTIAN_BRUSH,
    )?;
    m.add(
        "DEFAULT_VALUE_LINTIAN_BRUSH_ADDON_ONLY",
        lintian_brush::DEFAULT_VALUE_LINTIAN_BRUSH_ADDON_ONLY,
    )?;
    m.add(
        "LINTIAN_BRUSH_TAG_DEFAULT_VALUE",
        lintian_brush::LINTIAN_BRUSH_TAG_DEFAULT_VALUE,
    )?;
    m.add(
        "DEFAULT_ADDON_FIXERS",
        PyList::new(py, lintian_brush::DEFAULT_ADDON_FIXERS),
    )?;
    let tag_values = PyDict::new(py);
    for (k, v) in lintian_brush::LINTIAN_BRUSH_TAG_VALUES.iter() {
        tag_values.set_item(k, v)?;
    }
    m.add("LINTIAN_BRUSH_TAG_VALUES", tag_values)?;

    let multiarch_m = PyModule::new(py, "multiarch_hints")?;
    multiarch_m.add_wrapped(wrap_pyfunction!(calculate_multiarch_value))?;
    multiarch_m.add("MULTIARCH_HINTS_URL", multiarch_hints::MULTIARCH_HINTS_URL)?;
    multiarch_m.add_wrapped(wrap_pyfunction!(cache_download_multiarch_hints))?;
    multiarch_m.add_wrapped(wrap_pyfunction!(download_multiarch_hints))?;
    m.add_submodule(multiarch_m)?;
    m.add_function(wrap_pyfunction!(report_fatal, m)?)?;
    m.add_function(wrap_pyfunction!(report_success, m)?)?;
    m.add_function(wrap_pyfunction!(report_success_debian, m)?)?;
    m.add(
        "PACKAGE_CONFIG_FILENAME",
        lintian_brush::config::PACKAGE_CONFIG_FILENAME,
    )?;
    m.add_class::<Config>()?;
    Ok(())
}
