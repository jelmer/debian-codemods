use debversion::Version;
use pyo3::class::basic::CompareOp;
use pyo3::create_exception;
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::import_exception;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyFloat, PyList, PyString};
use std::collections::HashMap;

import_exception!(debmutate.reformatting, FormattingUnpreservable);
import_exception!(lintian_brush, NoChanges);
import_exception!(lintian_brush, DescriptionMissing);
import_exception!(lintian_brush, NotCertainEnough);
import_exception!(lintian_brush, FixerScriptFailed);
import_exception!(lintian_brush, NotDebianPackage);
create_exception!(lintian_brush, ScriptNotFound, pyo3::exceptions::PyException);
create_exception!(
    lintian_brush,
    UnsupportedCertainty,
    pyo3::exceptions::PyException
);

#[pyclass(subclass, unsendable, frozen)]
pub struct Fixer(pub Box<dyn crate::Fixer>);

#[pymethods]
impl Fixer {
    #[getter]
    fn name(&self) -> PyResult<String> {
        Ok(self.0.name())
    }

    #[getter]
    fn script_path(&self) -> PyResult<std::path::PathBuf> {
        Ok(self.0.path())
    }

    #[getter]
    fn lintian_tags(&self) -> PyResult<Vec<String>> {
        Ok(self
            .0
            .lintian_tags()
            .iter()
            .map(|s| s.to_string())
            .collect())
    }

    fn run(
        &self,
        py: Python,
        basedir: std::path::PathBuf,
        package: &str,
        current_version: Version,
        compat_release: &str,
        minimum_certainty: Option<&str>,
        trust_package: Option<bool>,
        allow_reformatting: Option<bool>,
        net_access: Option<bool>,
        opinionated: Option<bool>,
        diligence: Option<i32>,
    ) -> PyResult<FixerResult> {
        let minimum_certainty = minimum_certainty
            .map(|c| c.parse().map_err(UnsupportedCertainty::new_err))
            .transpose()?;

        self.0
            .run(
                basedir.as_path(),
                package,
                &current_version,
                compat_release,
                minimum_certainty,
                trust_package,
                allow_reformatting,
                net_access,
                opinionated,
                diligence,
            )
            .map_err(|e| match e {
                crate::FixerError::NoChanges => NoChanges::new_err((py.None(),)),
                crate::FixerError::ScriptNotFound(cmd) => {
                    ScriptNotFound::new_err(cmd.to_object(py))
                }
                crate::FixerError::ScriptFailed {
                    path,
                    exit_code,
                    stderr,
                } => FixerScriptFailed::new_err((path.to_object(py), exit_code, stderr)),
                crate::FixerError::FormattingUnpreservable => FormattingUnpreservable::new_err(()),
                crate::FixerError::OutputDecodeError(e) => {
                    PyValueError::new_err(format!("invalid output: {}", e))
                }
                crate::FixerError::OutputParseError(e) => match e {
                    crate::OutputParseError::LintianIssueParseError(e) => {
                        PyValueError::new_err(format!("invalid lintian issue: {}", e))
                    }
                    crate::OutputParseError::UnsupportedCertainty(e) => {
                        UnsupportedCertainty::new_err(e)
                    }
                },
                #[cfg(feature = "python")]
                crate::FixerError::Python(e) => e.into(),
                crate::FixerError::Other(e) => PyRuntimeError::new_err(e),
                crate::FixerError::NoChangesAfterOverrides(o) => NoChanges::new_err((py.None(),)),
                crate::FixerError::DescriptionMissing => DescriptionMissing::new_err(()),
                crate::FixerError::NotCertainEnough(certainty, minimum_certainty, os) => {
                    NotCertainEnough::new_err((
                        py.None(),
                        certainty.map(|c| c.to_string()),
                        minimum_certainty.map(|c| c.to_string()),
                    ))
                }
                crate::FixerError::NotDebianPackage(p) => NotDebianPackage::new_err(p),
            })
            .map(FixerResult)
    }

    fn __str__(&self) -> PyResult<String> {
        self.name()
    }
}

#[pyclass(extends=Fixer)]
pub struct ScriptFixer;

#[pymethods]
impl ScriptFixer {
    #[new]
    fn new(name: String, tags: Vec<String>, path: std::path::PathBuf) -> (Self, Fixer) {
        let fixer = crate::ScriptFixer::new(name, tags, path);
        (Self, Fixer(Box::new(fixer)))
    }
}

#[pyclass(extends=Fixer)]
pub struct PythonScriptFixer;

#[pymethods]
impl PythonScriptFixer {
    #[new]
    fn new(name: String, tags: Vec<String>, path: std::path::PathBuf) -> (Self, Fixer) {
        let fixer = crate::PythonScriptFixer::new(name, tags, path);
        (Self, Fixer(Box::new(fixer)))
    }
}

#[pyclass(subclass)]
pub struct FixerResult(pub crate::FixerResult);

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
                c.parse()
                    .map_err(|e| PyValueError::new_err(format!("invalid certainty: {}", e)))
            })
            .transpose()?;
        Ok(Self(crate::FixerResult::new(
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
    fn fixed_lintian_issues(&self) -> PyResult<Vec<LintianIssue>> {
        Ok(self
            .0
            .fixed_lintian_issues
            .iter()
            .map(|i| LintianIssue(i.clone()))
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
    fn overridden_lintian_issues(&self) -> PyResult<Vec<LintianIssue>> {
        Ok(self
            .0
            .overridden_lintian_issues
            .iter()
            .map(|i| LintianIssue(i.clone()))
            .collect())
    }
}

pub fn json_to_py(py: Python, v: serde_json::Value) -> PyResult<PyObject> {
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

#[pyclass]
pub struct ManyResult(crate::ManyResult);

pub fn py_to_json(py: Python, obj: PyObject) -> PyResult<serde_json::Value> {
    if obj.is_none(py) {
        return Ok(serde_json::Value::Null);
    }
    if let Ok(obj) = obj.extract::<bool>(py) {
        return Ok(serde_json::Value::Bool(obj));
    }
    if let Ok(obj) = obj.extract::<i64>(py) {
        return Ok(serde_json::Value::Number(obj.into()));
    }
    if let Ok(obj) = obj.extract::<u64>(py) {
        return Ok(serde_json::Value::Number(obj.into()));
    }
    if let Ok(obj) = obj.extract::<f64>(py) {
        return Ok(serde_json::json!(obj));
    }
    if let Ok(obj) = obj.extract::<String>(py) {
        return Ok(serde_json::Value::String(obj));
    }
    if let Ok(obj) = obj.extract::<Vec<PyObject>>(py) {
        let items: Vec<serde_json::Value> = obj
            .into_iter()
            .map(|o| py_to_json(py, o))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(serde_json::Value::Array(items));
    }
    if let Ok(obj) = obj.extract::<HashMap<String, PyObject>>(py) {
        let items: serde_json::Map<String, serde_json::Value> = obj
            .into_iter()
            .map(|(k, v)| Ok((k, py_to_json(py, v)?)))
            .collect::<PyResult<serde_json::Map<_, _>>>()?;
        return Ok(serde_json::Value::Object(items));
    }
    Err(PyTypeError::new_err("invalid type"))
}

#[pyclass(subclass)]
pub struct LintianIssue(crate::LintianIssue);

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
