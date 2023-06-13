use pyo3::class::basic::CompareOp;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyFloat, PyList, PyString};
use std::convert::TryInto;

use pyo3::create_exception;

create_exception!(
    lintian_brush,
    UnsupportedCertainty,
    pyo3::exceptions::PyException
);

#[pyclass(subclass)]
struct LintianIssue(lintian_brush::LintianIssue);

fn json_to_py(py: Python, v: serde_json::Value) -> PyResult<PyObject> {
    match v {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok(PyBool::new(py, b).to_object(py)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_py(py))
            } else if let Some(u) = n.as_u64() {
                Ok(u.into_py(py))
            } else if let Some(f) = n.as_f64() {
                Ok(PyFloat::new(py, f).to_object(py))
            } else {
                Err(PyTypeError::new_err("invalid number"))?
            }
        }
        serde_json::Value::String(s) => Ok(PyString::new(py, &s).to_object(py)),
        serde_json::Value::Array(a) => {
            let list = PyList::empty(py);
            for v in a {
                list.append(json_to_py(py, v)?)?;
            }
            Ok(list.to_object(py))
        }
        serde_json::Value::Object(o) => {
            let dict = PyDict::new(py);
            for (k, v) in o {
                dict.set_item(k, json_to_py(py, v)?)?;
            }
            Ok(dict.to_object(py))
        }
    }
}

#[pymethods]
impl LintianIssue {
    fn json(&self, py: Python) -> PyResult<PyObject> {
        json_to_py(py, self.0.json())
    }

    #[getter]
    fn package(&self) -> PyResult<Option<String>> {
        Ok(self.0.package.clone())
    }

    #[getter]
    fn package_type(&self) -> PyResult<Option<String>> {
        Ok(self.0.package_type.as_ref().map(|t| t.to_string()))
    }

    #[getter]
    fn tag(&self) -> PyResult<Option<String>> {
        Ok(self.0.tag.clone())
    }

    #[getter]
    fn info(&self) -> PyResult<Option<Vec<String>>> {
        Ok(self.0.info.clone())
    }

    fn __richcmp__(&self, other: PyRef<Self>, op: CompareOp) -> PyResult<bool> {
        match op {
            CompareOp::Eq => Ok(self.0 == other.0),
            CompareOp::Ne => Ok(self.0 != other.0),
            _ => Err(PyTypeError::new_err("invalid comparison")),
        }
    }
}

#[pyclass(subclass)]
struct FixerResult(lintian_brush::FixerResult);

#[pymethods]
impl FixerResult {
    #[new]
    fn new(
        description: String,
        fixed_lintian_tags: Option<Vec<String>>,
        certainty: Option<String>,
        patch_name: Option<String>,
        revision_id: Option<Vec<u8>>,
        fixed_lintian_issues: Option<Vec<PyRef<LintianIssue>>>,
        overridden_lintian_issues: Option<Vec<PyRef<LintianIssue>>>,
    ) -> PyResult<Self> {
        let certainty = certainty
            .map(|c| {
                c.as_str()
                    .try_into()
                    .map_err(|e| PyValueError::new_err(format!("invalid certainty: {}", e)))
            })
            .transpose()?;
        Ok(Self(lintian_brush::FixerResult::new(
            description,
            fixed_lintian_tags,
            certainty,
            patch_name,
            revision_id.map(|r| r.into()),
            fixed_lintian_issues
                .map(|i| i.iter().map(|i| i.0.clone()).collect())
                .unwrap_or_default(),
            overridden_lintian_issues.map(|i| i.iter().map(|i| i.0.clone()).collect()),
        )))
    }

    #[getter]
    fn fixed_lintian_tags(&self) -> PyResult<Vec<String>> {
        Ok(self
            .0
            .fixed_lintian_tags()
            .iter()
            .map(|s| s.to_string())
            .collect())
    }

    #[getter]
    fn description(&self) -> PyResult<String> {
        Ok(self.0.description.clone())
    }

    #[getter]
    fn certainty(&self) -> PyResult<Option<String>> {
        Ok(self.0.certainty.as_ref().map(|c| c.to_string()))
    }

    #[getter]
    fn patch_name(&self) -> PyResult<Option<String>> {
        Ok(self.0.patch_name.clone())
    }

    #[getter]
    fn revision_id(&self, py: Python) -> PyResult<Option<PyObject>> {
        Ok(self
            .0
            .revision_id
            .clone()
            .map(|r| PyBytes::new(py, r.as_bytes()).to_object(py)))
    }

    #[setter]
    fn set_revision_id(&mut self, revid: Option<Vec<u8>>) -> PyResult<()> {
        self.0.revision_id = revid.map(|r| r.into());
        Ok(())
    }

    fn __richcmp__(&self, other: PyRef<Self>, op: CompareOp) -> PyResult<bool> {
        match op {
            CompareOp::Eq => Ok(self.0 == other.0),
            CompareOp::Ne => Ok(self.0 != other.0),
            _ => Err(PyTypeError::new_err("invalid comparison")),
        }
    }

    #[getter]
    fn overridden_lintian_issues(&self, py: Python) -> PyResult<Vec<LintianIssue>> {
        Ok(self
            .0
            .overridden_lintian_issues
            .iter()
            .map(|i| LintianIssue(i.clone()))
            .collect())
    }
}

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

#[pymodule]
fn _lintian_brush_rs(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<LintianIssue>()?;
    m.add_class::<FixerResult>()?;
    m.add_wrapped(wrap_pyfunction!(parse_script_fixer_output))?;
    m.add(
        "UnsupportedCertainty",
        py.get_type::<UnsupportedCertainty>(),
    )?;
    Ok(())
}
