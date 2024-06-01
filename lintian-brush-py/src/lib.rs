use debian_analyzer::publish::Error as PublishError;
use pyo3::exceptions::{
    PyException, PyFileNotFoundError, PyMemoryError, PyRuntimeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple, PyType};
use pyo3::{create_exception, import_exception};

use std::collections::HashMap;

use debian_analyzer::Certainty;
use lintian_brush::py::{
    json_to_py, py_to_json, Fixer, FixerResult, LintianIssue, ManyResult, PythonScriptFixer,
    ScriptFixer, UnsupportedCertainty,
};

use debversion::Version;

import_exception!(debian.changelog, ChangelogCreateError);
import_exception!(debmutate.reformatting, FormattingUnpreservable);
import_exception!(lintian_brush, NoChanges);
import_exception!(lintian_brush, DescriptionMissing);
import_exception!(lintian_brush, NotCertainEnough);
import_exception!(lintian_brush, FixerScriptFailed);
import_exception!(lintian_brush, NotDebianPackage);
import_exception!(lintian_brush, ScriptNotFound);
import_exception!(lintian_brush, FailedPatchManipulation);
import_exception!(lintian_brush, WorkspaceDirty);

create_exception!(lintian_brush.publish, NoVcsLocation, PyException);
create_exception!(
    lintian_brush.publish,
    ConflictingVcsAlreadySpecified,
    PyException
);

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
fn determine_env(
    package: &str,
    current_version: Version,
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
        .map_err(UnsupportedCertainty::new_err)?;

    Ok(lintian_brush::determine_env(
        package,
        &current_version,
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
fn default_debianize_cache_dir() -> PyResult<std::path::PathBuf> {
    debianize::default_debianize_cache_dir().map_err(|e| PyValueError::new_err(e.to_string()))
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
    Ok(debian_analyzer::certainty_sufficient(
        actual_certainty,
        minimum_certainty,
    ))
}

#[pyfunction]
fn min_certainty(certainties: Vec<String>) -> PyResult<String> {
    let certainties = certainties
        .iter()
        .map(|c| c.parse().map_err(UnsupportedCertainty::new_err))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(debian_analyzer::min_certainty(certainties.as_slice())
        .unwrap_or(Certainty::Certain)
        .to_string())
}

#[pyfunction]
fn resolve_release_codename(name: &str, date: Option<chrono::NaiveDate>) -> Option<String> {
    debian_analyzer::release_info::resolve_release_codename(name, date)
}

#[pyfunction]
fn calculate_value(tags: Vec<String>) -> i32 {
    let tags = tags.iter().map(|s| s.as_str()).collect::<Vec<_>>();
    lintian_brush::calculate_value(tags.as_slice())
}

#[pyfunction]
fn report_fatal(
    versions: HashMap<String, String>,
    code: &str,
    description: &str,
    hint: Option<&str>,
    transient: Option<bool>,
) {
    debian_analyzer::svp::report_fatal(versions, code, description, hint, transient)
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

    debian_analyzer::svp::report_success(versions, value, context);
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
    debian_analyzer::svp::report_success_debian(versions, value, context, changelog);
    Ok(())
}

#[pyclass]
struct Config(debian_analyzer::config::Config);

#[pymethods]
impl Config {
    #[new]
    fn new(path: std::path::PathBuf) -> PyResult<Self> {
        Ok(Config(debian_analyzer::config::Config::load_from_path(
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
            .join(debian_analyzer::config::PACKAGE_CONFIG_FILENAME);
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

#[pyfunction]
pub fn load_resume(py: Python) -> PyResult<PyObject> {
    if let Some(resume) = debian_analyzer::svp::load_resume() {
        Ok(json_to_py(py, resume)?)
    } else {
        Ok(py.None())
    }
}

#[pyfunction]
fn increment_version(mut version: debversion::Version) -> PyResult<debversion::Version> {
    version.increment_debian();
    Ok(version)
}

#[pyfunction]
fn svp_enabled() -> bool {
    debian_analyzer::svp::enabled()
}

#[derive(Debug, Clone)]
struct PyFixer(PyObject);

impl lintian_brush::Fixer for PyFixer {
    fn name(&self) -> String {
        Python::with_gil(|py| self.0.getattr(py, "name").unwrap().extract(py).unwrap())
    }

    fn path(&self) -> std::path::PathBuf {
        Python::with_gil(|py| {
            self.0
                .getattr(py, "path")
                .unwrap()
                .extract::<std::path::PathBuf>(py)
                .unwrap()
        })
    }

    fn lintian_tags(&self) -> Vec<String> {
        Python::with_gil(|py| {
            self.0
                .getattr(py, "lintian_tags")
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn run(
        &self,
        basedir: &std::path::Path,
        package: &str,
        current_version: &Version,
        compat_release: &str,
        minimum_certainty: Option<Certainty>,
        trust_package: Option<bool>,
        allow_reformatting: Option<bool>,
        net_access: Option<bool>,
        opinionated: Option<bool>,
        diligence: Option<i32>,
    ) -> Result<lintian_brush::FixerResult, lintian_brush::FixerError> {
        Python::with_gil(|py| {
            let ob = self.0.call_method(
                py,
                "run",
                (
                    basedir,
                    package,
                    current_version.clone(),
                    compat_release,
                    minimum_certainty.map(|c| c.to_string()),
                    trust_package,
                    allow_reformatting,
                    net_access,
                    opinionated,
                    diligence,
                ),
                None,
            )?;
            let description = ob.getattr(py, "description")?.extract(py)?;
            let certainty = ob.getattr(py, "certainty")?.extract(py)?;
            let patch_name = ob.getattr(py, "patch_name")?.extract(py)?;
            let revision_id = ob.getattr(py, "revision_id")?.extract(py)?;
            let fixed_lintian_issues = ob.getattr(py, "fixed_lintian_issues")?.extract(py)?;
            let overridden_lintian_issues =
                ob.getattr(py, "overridden_lintian_issues")?.extract(py)?;
            let result = lintian_brush::FixerResult {
                description,
                certainty,
                patch_name,
                revision_id,
                fixed_lintian_issues,
                overridden_lintian_issues,
            };
            Ok(result)
        })
    }
}

#[pyfunction]
fn run_lintian_fixer(
    py: Python,
    local_tree: PyObject,
    fixer: PyObject,
    committer: Option<&str>,
    update_changelog: Option<PyObject>,
    compat_release: Option<&str>,
    minimum_certainty: Option<Certainty>,
    trust_package: Option<bool>,
    allow_reformatting: Option<bool>,
    dirty_tracker: Option<PyObject>,
    subpath: Option<std::path::PathBuf>,
    net_access: Option<bool>,
    opinionated: Option<bool>,
    diligence: Option<i32>,
    timestamp: Option<chrono::naive::NaiveDateTime>,
    basis_tree: Option<PyObject>,
    changes_by: Option<&str>,
) -> PyResult<(FixerResult, String)> {
    let subpath = subpath.unwrap_or_else(|| "".into());

    let update_changelog = || -> bool {
        update_changelog.clone().map_or(false, |u| {
            pyo3::Python::with_gil(|py| {
                if u.as_ref(py).is_callable() {
                    u.call0(py).unwrap().extract(py).unwrap()
                } else {
                    u.extract(py).unwrap()
                }
            })
        })
    };

    let core_fixer;

    let fixer: &Box<dyn lintian_brush::Fixer> =
        if let Ok(fixer) = fixer.extract::<&PyCell<Fixer>>(py) {
            &fixer.get().0
        } else {
            core_fixer = Some(Box::new(PyFixer(fixer)) as Box<dyn lintian_brush::Fixer>);
            core_fixer.as_ref().unwrap()
        };

    lintian_brush::run_lintian_fixer(
        &breezyshim::WorkingTree(local_tree),
        fixer,
        committer,
        update_changelog,
        compat_release,
        minimum_certainty,
        trust_package,
        allow_reformatting,
        dirty_tracker.map(breezyshim::DirtyTracker::from).as_ref(),
        subpath.as_path(),
        net_access,
        opinionated,
        diligence,
        timestamp,
        basis_tree
            .map(|bt| Box::new(breezyshim::RevisionTree(bt)) as Box<dyn breezyshim::Tree>)
            .as_ref(),
        changes_by,
    )
    .map_err(|e| match e {
        lintian_brush::FixerError::NoChanges => NoChanges::new_err((py.None(),)),
        lintian_brush::FixerError::ChangelogCreate(m) => ChangelogCreateError::new_err((m,)),
        lintian_brush::FixerError::ScriptNotFound(cmd) => {
            ScriptNotFound::new_err(cmd.to_object(py))
        }
        lintian_brush::FixerError::ScriptFailed {
            path,
            exit_code,
            stderr,
        } => FixerScriptFailed::new_err((path.to_object(py), exit_code, stderr)),
        lintian_brush::FixerError::FormattingUnpreservable(p) => {
            FormattingUnpreservable::new_err((p,))
        }
        lintian_brush::FixerError::OutputDecodeError(e) => {
            PyValueError::new_err(format!("invalid output: {}", e))
        }
        lintian_brush::FixerError::OutputParseError(e) => match e {
            lintian_brush::OutputParseError::LintianIssueParseError(e) => {
                PyValueError::new_err(format!("invalid lintian issue: {}", e))
            }
            lintian_brush::OutputParseError::UnsupportedCertainty(e) => {
                UnsupportedCertainty::new_err(e)
            }
        },
        #[cfg(feature = "python")]
        lintian_brush::FixerError::Python(e) => e.into(),
        lintian_brush::FixerError::Io(e) => e.into(),
        lintian_brush::FixerError::Other(e) => PyRuntimeError::new_err(e),
        lintian_brush::FixerError::NoChangesAfterOverrides(_o) => NoChanges::new_err((py.None(),)),
        lintian_brush::FixerError::DescriptionMissing => DescriptionMissing::new_err(()),
        lintian_brush::FixerError::NotCertainEnough(certainty, minimum_certainty, _) => {
            NotCertainEnough::new_err((
                py.None(),
                certainty.to_string(),
                minimum_certainty.map(|c| c.to_string()),
            ))
        }
        lintian_brush::FixerError::NotDebianPackage(e) => NotDebianPackage::new_err(e),
        lintian_brush::FixerError::Python(e) => e,
        lintian_brush::FixerError::FailedPatchManipulation(p1, p2, reason) => {
            FailedPatchManipulation::new_err((p1, p2, reason))
        }
        lintian_brush::FixerError::MemoryError => PyMemoryError::new_err(()),
        lintian_brush::FixerError::TreeError(e) => e.into(),
        lintian_brush::FixerError::InvalidChangelog(p, s) => ChangelogCreateError::new_err((p, s)),
    })
    .map(|(result, output)| (FixerResult(result), output))
}

#[pyfunction]
fn only_changes_last_changelog_block(
    tree: PyObject,
    basis_tree: PyObject,
    changelog_path: std::path::PathBuf,
    changes: Vec<breezyshim::tree::TreeChange>,
) -> pyo3::PyResult<bool> {
    let tree = breezyshim::WorkingTree(tree);
    let basis_tree = Box::new(breezyshim::RevisionTree(basis_tree)) as Box<dyn breezyshim::Tree>;
    let changelog_path = changelog_path.as_path();
    debian_analyzer::changelog::only_changes_last_changelog_block(
        &tree,
        basis_tree.as_ref(),
        changelog_path,
        changes.iter(),
    )
    .map_err(|e| PyValueError::new_err((e.to_string(),)))
}

#[pyfunction]
fn control_file_present(tree: PyObject, path: std::path::PathBuf) -> pyo3::PyResult<bool> {
    let tree = breezyshim::tree::RevisionTree(tree);
    let path = path.as_path();
    Ok(debian_analyzer::control_file_present(&tree, path))
}

#[pyfunction]
fn control_files_in_root(tree: PyObject, path: std::path::PathBuf) -> pyo3::PyResult<bool> {
    let tree = breezyshim::tree::RevisionTree(tree);
    let path = path.as_path();
    Ok(debian_analyzer::control_files_in_root(&tree, path))
}

#[pyfunction]
fn is_debcargo_package(tree: PyObject, path: std::path::PathBuf) -> pyo3::PyResult<bool> {
    let tree = breezyshim::tree::RevisionTree(tree);
    let path = path.as_path();
    Ok(debian_analyzer::is_debcargo_package(&tree, path))
}

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
        Ok((repo_url, branch, subpath)) => Ok((repo_url.to_string(), branch, subpath)),
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
fn find_fixers_dir() -> Option<std::path::PathBuf> {
    lintian_brush::find_fixers_dir()
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
    m.add_wrapped(wrap_pyfunction!(available_lintian_fixers))?;
    m.add_wrapped(wrap_pyfunction!(certainty_sufficient))?;
    m.add_wrapped(wrap_pyfunction!(min_certainty))?;
    m.add_wrapped(wrap_pyfunction!(resolve_release_codename))?;
    m.add_wrapped(wrap_pyfunction!(calculate_value))?;
    m.add_wrapped(wrap_pyfunction!(find_fixers_dir))?;
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

    m.add_function(wrap_pyfunction!(report_fatal, m)?)?;
    m.add_function(wrap_pyfunction!(report_success, m)?)?;
    m.add_function(wrap_pyfunction!(report_success_debian, m)?)?;
    m.add(
        "PACKAGE_CONFIG_FILENAME",
        debian_analyzer::config::PACKAGE_CONFIG_FILENAME,
    )?;
    m.add_class::<Config>()?;
    m.add_wrapped(wrap_pyfunction!(increment_version))?;
    m.add_wrapped(wrap_pyfunction!(load_resume))?;
    m.add_wrapped(wrap_pyfunction!(svp_enabled))?;
    m.add_class::<ManyResult>()?;
    m.add_function(wrap_pyfunction!(run_lintian_fixer, m)?)?;
    m.add_function(wrap_pyfunction!(only_changes_last_changelog_block, m)?)?;
    m.add_function(wrap_pyfunction!(control_file_present, m)?)?;
    m.add_function(wrap_pyfunction!(control_files_in_root, m)?)?;
    m.add_function(wrap_pyfunction!(is_debcargo_package, m)?)?;
    let v = PyTuple::new(
        py,
        env!("CARGO_PKG_VERSION")
            .split('.')
            .map(|x| x.parse::<u32>().unwrap())
            .collect::<Vec<u32>>(),
    );
    m.add("__version__", v)?;
    m.add_class::<ChangelogBehaviour>()?;
    m.add_wrapped(wrap_pyfunction!(guess_update_changelog))?;
    m.add_wrapped(wrap_pyfunction!(update_official_vcs))?;
    m.add_wrapped(wrap_pyfunction!(guess_repository_url))?;
    m.add_wrapped(wrap_pyfunction!(default_debianize_cache_dir))?;
    m.add("NoVcsLocation", py.get_type::<NoVcsLocation>())?;
    m.add(
        "ConflictingVcsAlreadySpecified",
        py.get_type::<ConflictingVcsAlreadySpecified>(),
    )?;
    Ok(())
}
