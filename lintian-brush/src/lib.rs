use debversion::Version;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::process::Command;
use std::str::FromStr;

use indicatif::ProgressBar;

use breezyshim::dirty_tracker::DirtyTreeTracker;
use breezyshim::error::Error;
use breezyshim::tree::{Tree, TreeChange, WorkingTree};
use breezyshim::workspace::{check_clean_tree, reset_tree_with_dirty_tracker};
use breezyshim::RevisionId;
use debian_analyzer::detect_gbp_dch::{guess_update_changelog, ChangelogBehaviour};
use debian_analyzer::{
    add_changelog_entry, apply_or_revert, certainty_sufficient, get_committer, min_certainty,
    ApplyError, Certainty, ChangelogError,
};
use debian_changelog::ChangeLog;

#[cfg(feature = "python")]
pub mod py;

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum PackageType {
    #[serde(rename = "source")]
    Source,
    #[serde(rename = "binary")]
    Binary,
}

impl FromStr for PackageType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "source" => Ok(PackageType::Source),
            "binary" => Ok(PackageType::Binary),
            _ => Err(format!("Invalid package type: {}", value)),
        }
    }
}

impl std::fmt::Display for PackageType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PackageType::Source => write!(f, "source"),
            PackageType::Binary => write!(f, "binary"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct LintianIssue {
    pub package: Option<String>,
    pub package_type: Option<PackageType>,
    pub tag: Option<String>,
    pub info: Option<Vec<String>>,
}

impl LintianIssue {
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "package": self.package,
            "package_type": self.package_type.as_ref().map(|t| t.to_string()),
            "tag": self.tag,
            "info": self.info,
        })
    }

    pub fn from_json(value: serde_json::Value) -> serde_json::Result<Self> {
        serde_json::from_value(value)
    }

    pub fn just_tag(tag: String) -> Self {
        Self {
            package: None,
            package_type: None,
            tag: Some(tag),
            info: None,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LintianIssueParseError {
    InvalidPackageType(String),
}

#[cfg(feature = "python")]
impl pyo3::FromPyObject<'_> for LintianIssue {
    fn extract_bound(ob: &pyo3::Bound<pyo3::PyAny>) -> pyo3::PyResult<Self> {
        use pyo3::prelude::*;
        let package = ob.getattr("package")?.extract::<Option<String>>()?;
        let package_type = ob
            .getattr("package_type")?
            .extract::<Option<String>>()?
            .map(|s| {
                s.parse::<PackageType>()
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err((e,)))
            })
            .transpose()?;
        let tag = ob.getattr("tag")?.extract::<Option<String>>()?;
        let info = ob.getattr("info")?.extract::<Option<Vec<String>>>()?;
        Ok(Self {
            package,
            package_type,
            tag,
            info,
        })
    }
}

impl std::fmt::Display for LintianIssueParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LintianIssueParseError::InvalidPackageType(s) => {
                write!(f, "Invalid package type: {}", s)
            }
        }
    }
}

impl std::error::Error for LintianIssueParseError {}

impl TryFrom<&str> for LintianIssue {
    type Error = LintianIssueParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let value = value.trim();
        let package_type;
        let package;
        let after = if let Some((before, after)) = value.split_once(':') {
            if let Some((package_type_str, package_str)) = before.trim().split_once(' ') {
                package_type = Some(match package_type_str {
                    "source" => PackageType::Source,
                    "binary" => PackageType::Binary,
                    _ => {
                        return Err(LintianIssueParseError::InvalidPackageType(
                            package_type_str.to_string(),
                        ))
                    }
                });
                package = Some(package_str.to_string());
            } else {
                package_type = None;
                package = Some(before.to_string());
            }
            after
        } else {
            package_type = None;
            package = None;
            value
        };
        let mut parts = after.trim().split(' ');
        let tag = parts.next().map(|s| s.to_string());
        let info: Vec<_> = parts.map(|s| s.to_string()).collect();
        let info = if info.is_empty() { None } else { Some(info) };
        Ok(Self {
            package,
            package_type,
            tag,
            info,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct FixerResult {
    pub description: String,
    pub certainty: Option<Certainty>,
    pub patch_name: Option<String>,
    pub revision_id: Option<RevisionId>,
    pub fixed_lintian_issues: Vec<LintianIssue>,
    pub overridden_lintian_issues: Vec<LintianIssue>,
}

impl FixerResult {
    pub fn new(
        description: String,
        fixed_lintian_tags: Option<Vec<String>>,
        certainty: Option<Certainty>,
        patch_name: Option<String>,
        revision_id: Option<RevisionId>,
        mut fixed_lintian_issues: Vec<LintianIssue>,
        overridden_lintian_issues: Option<Vec<LintianIssue>>,
    ) -> Self {
        if let Some(fixed_lintian_tags) = fixed_lintian_tags.as_ref() {
            fixed_lintian_issues.extend(
                fixed_lintian_tags
                    .iter()
                    .map(|tag| LintianIssue::just_tag(tag.to_string())),
            );
        }
        Self {
            description,
            certainty,
            patch_name,
            revision_id,
            fixed_lintian_issues,
            overridden_lintian_issues: overridden_lintian_issues.unwrap_or_default(),
        }
    }
    pub fn fixed_lintian_tags(&self) -> Vec<&str> {
        self.fixed_lintian_issues
            .iter()
            .filter_map(|issue| issue.tag.as_deref())
            .collect()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OutputParseError {
    UnsupportedCertainty(String),
    LintianIssueParseError(LintianIssueParseError),
}

impl std::fmt::Display for OutputParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            OutputParseError::UnsupportedCertainty(s) => {
                write!(f, "Unsupported certainty: {}", s)
            }
            OutputParseError::LintianIssueParseError(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for OutputParseError {}

impl From<LintianIssueParseError> for OutputParseError {
    fn from(value: LintianIssueParseError) -> Self {
        Self::LintianIssueParseError(value)
    }
}

pub fn parse_script_fixer_output(text: &str) -> Result<FixerResult, OutputParseError> {
    let mut description: Vec<String> = Vec::new();
    let mut overridden_issues: Vec<LintianIssue> = Vec::new();
    let mut fixed_lintian_issues: Vec<LintianIssue> = Vec::new();
    let mut fixed_lintian_tags: Vec<String> = Vec::new();
    let mut certainty: Option<String> = None;
    let mut patch_name: Option<String> = None;

    let lines: Vec<&str> = text.split_terminator('\n').collect();
    let mut i = 0;

    while i < lines.len() {
        if let Some((key, value)) = lines[i].split_once(':') {
            match key.trim() {
                "Fixed-Lintian-Tags" => {
                    fixed_lintian_tags.extend(value.split(',').map(|tag| tag.trim().to_owned()));
                }
                "Fixed-Lintian-Issues" => {
                    i += 1;
                    while i < lines.len() && lines[i].starts_with(' ') {
                        fixed_lintian_issues.push(LintianIssue::try_from(&lines[i][1..])?);
                        i += 1;
                    }
                    continue;
                }
                "Overridden-Lintian-Issues" => {
                    i += 1;
                    while i < lines.len() && lines[i].starts_with(' ') {
                        overridden_issues.push(LintianIssue::try_from(&lines[i][1..])?);
                        i += 1;
                    }
                    continue;
                }
                "Certainty" => {
                    certainty = Some(value.trim().to_owned());
                }
                "Patch-Name" => {
                    patch_name = Some(value.trim().to_owned());
                }
                _ => {
                    description.push(lines[i].to_owned());
                }
            }
        } else {
            description.push(lines[i].to_owned());
        }

        i += 1;
    }

    let certainty = certainty
        .map(|c| c.parse())
        .transpose()
        .map_err(OutputParseError::UnsupportedCertainty)?;

    let fixed_lintian_tags = if fixed_lintian_tags.is_empty() {
        None
    } else {
        Some(fixed_lintian_tags)
    };

    let overridden_issues = if overridden_issues.is_empty() {
        None
    } else {
        Some(overridden_issues)
    };

    Ok(FixerResult::new(
        description.join("\n"),
        fixed_lintian_tags,
        certainty,
        patch_name,
        None,
        fixed_lintian_issues,
        overridden_issues,
    ))
}

pub fn determine_env(
    package: &str,
    current_version: &Version,
    preferences: &FixerPreferences,
) -> std::collections::HashMap<String, String> {
    let mut env = std::env::vars().collect::<std::collections::HashMap<_, _>>();
    env.insert("DEB_SOURCE".to_owned(), package.to_owned());
    env.insert("CURRENT_VERSION".to_owned(), current_version.to_string());
    env.insert(
        "COMPAT_RELEASE".to_owned(),
        preferences
            .compat_release
            .as_deref()
            .unwrap_or("sid")
            .to_owned(),
    );
    env.insert(
        "MINIMUM_CERTAINTY".to_owned(),
        preferences
            .minimum_certainty
            .unwrap_or_default()
            .to_string(),
    );
    env.insert(
        "TRUST_PACKAGE".to_owned(),
        preferences.trust_package.unwrap_or(false).to_string(),
    );
    env.insert(
        "REFORMATTING".to_owned(),
        if preferences.allow_reformatting.unwrap_or(false) {
            "allow"
        } else {
            "disallow"
        }
        .to_owned(),
    );
    env.insert(
        "NET_ACCESS".to_owned(),
        if preferences.net_access.unwrap_or(true) {
            "allow"
        } else {
            "disallow"
        }
        .to_owned(),
    );
    env.insert(
        "OPINIONATED".to_owned(),
        if preferences.opinionated.unwrap_or(false) {
            "yes"
        } else {
            "no"
        }
        .to_owned(),
    );
    env.insert(
        "DILIGENCE".to_owned(),
        preferences.diligence.unwrap_or(0).to_string(),
    );
    env
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FixerPreferences {
    pub compat_release: Option<String>,
    pub minimum_certainty: Option<Certainty>,
    pub trust_package: Option<bool>,
    pub allow_reformatting: Option<bool>,
    pub net_access: Option<bool>,
    pub opinionated: Option<bool>,
    pub diligence: Option<i32>,
}

/// A fixer script
///
/// The `lintian_tags attribute contains the name of the lintian tags this fixer addresses.
pub trait Fixer: std::fmt::Debug + Sync {
    /// Name of the fixer
    fn name(&self) -> String;

    /// Path to the fixer script
    fn path(&self) -> std::path::PathBuf;

    /// Lintian tags this fixer addresses
    fn lintian_tags(&self) -> Vec<String>;

    /// Apply this fixer script.
    ///
    /// # Arguments
    ///
    /// * `basedir` - Directory in which to run
    /// * `package` - Name of the source package
    /// * `current_version` - The version of the package that is being created or updated
    /// * `compat_release` - Compatibility level (a Debian release name)
    /// * `minimum_certainty` - Minimum certainty level
    /// * `trust_package` - Whether to run code from the package
    /// * `allow_reformatting` - Allow reformatting of files that are being changed
    /// * `net_access` - Allow network access
    /// * `opinionated` - Whether to be opinionated
    /// * `diligence` - Level of diligence
    /// * `timeout` - Maximum time to run the fixer
    ///
    /// # Returns
    ///
    ///  A FixerResult object
    fn run(
        &self,
        basedir: &std::path::Path,
        package: &str,
        current_version: &Version,
        preferences: &FixerPreferences,
        timeout: Option<chrono::Duration>,
    ) -> Result<FixerResult, FixerError>;
}

/// A fixer that is implemented as a Python script.
///
/// This gets used just for Python scripts, and significantly speeds things up because it prevents
/// starting a new Python interpreter for every fixer.
#[cfg(feature = "python")]
#[derive(Debug)]
pub struct PythonScriptFixer {
    path: std::path::PathBuf,
    name: String,
    lintian_tags: Vec<String>,
}

#[cfg(feature = "python")]
impl PythonScriptFixer {
    pub fn new(name: String, lintian_tags: Vec<String>, path: std::path::PathBuf) -> Self {
        Self {
            path,
            name,
            lintian_tags,
        }
    }
}

#[cfg(feature = "python")]
fn run_inline_python_fixer(
    path: &std::path::Path,
    name: &str,
    code: &str,
    basedir: &std::path::Path,
    env: HashMap<String, String>,
    timeout: Option<chrono::Duration>,
) -> Result<FixerResult, FixerError> {
    pyo3::prepare_freethreaded_python();

    use pyo3::import_exception;
    use pyo3::prelude::*;
    use pyo3::types::PyDict;

    import_exception!(debmutate.reformatting, FormattingUnpreservable);
    import_exception!(debian.changelog, ChangelogCreateError);

    Python::with_gil(|py| {
        let sys = py.import_bound("sys")?;
        let os = py.import_bound("os")?;
        let io = py.import_bound("io")?;
        let fixer_module = py.import_bound("lintian_brush.fixer")?;

        let old_env = os.getattr("environ")?.into_py(py);
        let old_stderr = sys.getattr("stderr")?;
        let old_stdout = sys.getattr("stdout")?;

        let temp_stderr = io.call_method0("StringIO")?;
        let temp_stdout = io.call_method0("StringIO")?;

        sys.setattr("stderr", &temp_stderr)?;
        sys.setattr("stdout", &temp_stdout)?;
        os.setattr("environ", env)?;

        let old_cwd = match os.call_method0("getcwd") {
            Ok(cwd) => Some(cwd),
            Err(_) => None,
        };

        os.call_method1("chdir", (basedir,))?;

        let global_vars = PyDict::new_bound(py);
        global_vars.set_item("__file__", path)?;
        global_vars.set_item("__name__", "__main__")?;

        let script_result = PyModule::from_code_bound(py, code, path.to_str().unwrap(), name);

        let stdout = temp_stdout
            .call_method0("getvalue")
            .unwrap()
            .extract::<String>()
            .unwrap();

        let mut stderr = temp_stderr
            .call_method0("getvalue")
            .unwrap()
            .extract::<String>()
            .unwrap();

        os.setattr("environ", old_env).unwrap();
        sys.setattr("stderr", old_stderr).unwrap();
        sys.setattr("stdout", old_stdout).unwrap();

        if let Some(cwd) = old_cwd {
            os.call_method1("chdir", (cwd,))?;
        }

        fixer_module.call_method0("reset")?;

        let retcode;
        let description;

        match script_result {
            Ok(_) => {
                retcode = 0;
                description = stdout;
            }
            Err(e) => {
                if e.is_instance_of::<FormattingUnpreservable>(py) {
                    return Err(FixerError::FormattingUnpreservable(
                        e.into_value(py).bind(py).getattr("path")?.extract()?,
                    ));
                } else if e.is_instance_of::<ChangelogCreateError>(py) {
                    return Err(FixerError::ChangelogCreate(
                        e.into_value(py).bind(py).get_item(0)?.extract()?,
                    ));
                } else if e.is_instance_of::<pyo3::exceptions::PyMemoryError>(py) {
                    return Err(FixerError::MemoryError);
                } else if e.is_instance_of::<pyo3::exceptions::PySystemExit>(py) {
                    retcode = e.into_value(py).bind(py).getattr("code")?.extract()?;
                    description = stdout;
                } else {
                    use pyo3::types::IntoPyDict;
                    let traceback = py.import_bound("traceback")?;
                    let traceback_io = io.call_method0("StringIO")?;
                    let kwargs = [("file", &traceback_io)].into_py_dict_bound(py);
                    traceback.call_method(
                        "print_exception",
                        (e.get_type_bound(py), &e, e.traceback_bound(py)),
                        Some(&kwargs),
                    )?;
                    let traceback_str =
                        traceback_io.call_method0("getvalue")?.extract::<String>()?;
                    stderr = format!("{}\n{}", stderr, traceback_str);
                    return Err(FixerError::ScriptFailed {
                        path: path.to_path_buf(),
                        exit_code: 1,
                        stderr,
                    });
                }
            }
        }

        if retcode == 2 {
            Err(FixerError::NoChanges)
        } else if retcode != 0 {
            Err(FixerError::ScriptFailed {
                path: path.to_path_buf(),
                exit_code: retcode,
                stderr,
            })
        } else {
            Ok(parse_script_fixer_output(&description)?)
        }
    })
}

#[cfg(test)]
#[cfg(feature = "python")]
mod run_inline_python_fixer_tests {
    #[test]
    fn test_no_changes() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("no_changes.py");
        let result = super::run_inline_python_fixer(
            &path,
            "no_changes",
            "import sys; sys.exit(2)",
            td.path(),
            std::collections::HashMap::new(),
            None,
        );
        assert!(
            matches!(result, Err(super::FixerError::NoChanges),),
            "Result: {:?}",
            result
        );
    }

    #[test]
    fn test_failed() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("no_changes.py");
        let result = super::run_inline_python_fixer(
            &path,
            "some_changes",
            "import sys; sys.exit(1)",
            td.path(),
            std::collections::HashMap::new(),
            None,
        );
        assert!(matches!(
            result,
            Err(super::FixerError::ScriptFailed { exit_code: 1, .. })
        ));
        std::mem::drop(td);
    }

    #[test]
    #[ignore]
    fn test_timeout() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("no_changes.py");
        let result = super::run_inline_python_fixer(
            &path,
            "some_changes",
            "import time; time.sleep(10)",
            td.path(),
            std::collections::HashMap::new(),
            Some(chrono::Duration::seconds(0)),
        );
        assert!(
            matches!(result, Err(super::FixerError::Timeout { .. })),
            "Result: {:?}",
            result
        );
    }
}

#[cfg(feature = "python")]
impl Fixer for PythonScriptFixer {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn path(&self) -> std::path::PathBuf {
        self.path.clone()
    }

    fn lintian_tags(&self) -> Vec<String> {
        self.lintian_tags.clone()
    }

    fn run(
        &self,
        basedir: &std::path::Path,
        package: &str,
        current_version: &Version,
        preferences: &FixerPreferences,
        timeout: Option<chrono::Duration>,
    ) -> Result<FixerResult, FixerError> {
        let env = determine_env(package, current_version, preferences);

        let code = std::fs::read_to_string(&self.path)
            .map_err(|e| FixerError::Other(format!("Failed to read script: {}", e)))?;

        run_inline_python_fixer(
            &self.path,
            self.name.as_str(),
            code.as_str(),
            basedir,
            env,
            timeout,
        )
    }
}

#[derive(Debug)]
pub enum FixerError {
    NoChanges,
    NoChangesAfterOverrides(Vec<LintianIssue>),
    NotCertainEnough(Certainty, Option<Certainty>, Vec<LintianIssue>),
    NotDebianPackage(std::path::PathBuf),
    DescriptionMissing,
    InvalidChangelog(std::path::PathBuf, String),
    ScriptNotFound(std::path::PathBuf),
    OutputParseError(OutputParseError),
    OutputDecodeError(std::string::FromUtf8Error),
    FailedPatchManipulation(String),
    ChangelogCreate(String),
    Timeout {
        timeout: chrono::Duration,
    },
    ScriptFailed {
        path: std::path::PathBuf,
        exit_code: i32,
        stderr: String,
    },
    FormattingUnpreservable(std::path::PathBuf),
    GeneratedFile(std::path::PathBuf),
    #[cfg(feature = "python")]
    Python(pyo3::PyErr),
    MemoryError,
    Io(std::io::Error),
    BrzError(Error),
    Other(String),
}

impl From<debian_analyzer::editor::EditorError> for FixerError {
    fn from(e: debian_analyzer::editor::EditorError) -> Self {
        match e {
            debian_analyzer::editor::EditorError::IoError(e) => e.into(),
            debian_analyzer::editor::EditorError::BrzError(e) => e.into(),
            debian_analyzer::editor::EditorError::GeneratedFile(p, _) => {
                FixerError::GeneratedFile(p)
            }
            debian_analyzer::editor::EditorError::FormattingUnpreservable(p, _e) => {
                FixerError::FormattingUnpreservable(p)
            }
        }
    }
}

impl From<std::io::Error> for FixerError {
    fn from(e: std::io::Error) -> Self {
        FixerError::Io(e)
    }
}

impl From<debian_changelog::Error> for FixerError {
    fn from(e: debian_changelog::Error) -> Self {
        match e {
            debian_changelog::Error::Io(e) => FixerError::Io(e),
            debian_changelog::Error::Parse(e) => FixerError::ChangelogCreate(e.to_string()),
        }
    }
}

impl From<ChangelogError> for FixerError {
    fn from(e: ChangelogError) -> Self {
        match e {
            ChangelogError::NotDebianPackage(path) => FixerError::NotDebianPackage(path),
            ChangelogError::Python(e) => FixerError::Other(e.to_string()),
        }
    }
}

impl From<Error> for FixerError {
    fn from(e: Error) -> Self {
        FixerError::BrzError(e)
    }
}

impl From<OutputParseError> for FixerError {
    fn from(e: OutputParseError) -> Self {
        FixerError::OutputParseError(e)
    }
}

#[cfg(feature = "python")]
impl From<pyo3::PyErr> for FixerError {
    fn from(e: pyo3::PyErr) -> Self {
        FixerError::Python(e)
    }
}

impl std::fmt::Display for FixerError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FixerError::NoChanges => write!(f, "No changes"),
            FixerError::NoChangesAfterOverrides(_) => write!(f, "No changes after overrides"),
            FixerError::OutputParseError(e) => write!(f, "Output parse error: {}", e),
            FixerError::OutputDecodeError(e) => write!(f, "Output decode error: {}", e),
            FixerError::ScriptNotFound(p) => write!(f, "Command not found: {}", p.display()),
            FixerError::ChangelogCreate(m) => write!(f, "Changelog create error: {}", m),
            FixerError::FormattingUnpreservable(p) => {
                write!(f, "Formatting unpreservable for {}", p.display())
            }
            FixerError::ScriptFailed {
                path,
                exit_code,
                stderr,
            } => write!(
                f,
                "Script failed: {} (exit code {}) (stderr: {})",
                path.display(),
                exit_code,
                stderr
            ),
            FixerError::Other(s) => write!(f, "{}", s),
            #[cfg(feature = "python")]
            FixerError::Python(e) => write!(f, "{}", e),
            FixerError::NotDebianPackage(p) => write!(f, "Not a Debian package: {}", p.display()),
            FixerError::DescriptionMissing => {
                write!(f, "Description missing")
            }
            FixerError::MemoryError => {
                write!(f, "Memory error")
            }
            FixerError::NotCertainEnough(actual, minimum, _) => write!(
                f,
                "Not certain enough to fix (actual: {}, minimum : {:?})",
                actual, minimum
            ),
            FixerError::Io(e) => write!(f, "IO error: {}", e),
            FixerError::FailedPatchManipulation(s) => write!(f, "Failed to manipulate patc: {}", s),
            FixerError::BrzError(e) => write!(f, "Breezy error: {}", e),
            FixerError::InvalidChangelog(p, s) => {
                write!(f, "Invalid changelog {}: {}", p.display(), s)
            }
            FixerError::Timeout { timeout } => write!(f, "Timeout after {:?}", timeout),
            FixerError::GeneratedFile(p) => write!(f, "Generated file: {}", p.display()),
        }
    }
}

impl std::error::Error for FixerError {}

#[derive(Debug)]
pub struct ScriptFixer {
    path: std::path::PathBuf,
    name: String,
    lintian_tags: Vec<String>,
}

impl ScriptFixer {
    pub fn new(name: String, lintian_tags: Vec<String>, path: std::path::PathBuf) -> Self {
        Self {
            path,
            name,
            lintian_tags,
        }
    }
}

impl Fixer for ScriptFixer {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn path(&self) -> std::path::PathBuf {
        self.path.clone()
    }

    fn lintian_tags(&self) -> Vec<String> {
        self.lintian_tags.clone()
    }

    fn run(
        &self,
        basedir: &std::path::Path,
        package: &str,
        current_version: &Version,
        preferences: &FixerPreferences,
        timeout: Option<chrono::Duration>,
    ) -> Result<FixerResult, FixerError> {
        let env = determine_env(package, current_version, preferences);
        use wait_timeout::ChildExt;

        let mut cmd = Command::new(self.path.as_os_str());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.current_dir(basedir);

        for (key, value) in env.iter() {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => FixerError::ScriptNotFound(self.path.clone()),
            _ => FixerError::Other(e.to_string()),
        })?;

        let status = if let Some(timeout) = timeout {
            let std_timeout = timeout
                .to_std()
                .map_err(|e| FixerError::Other(e.to_string()))?;
            let output = child
                .wait_timeout(std_timeout)
                .map_err(|e| FixerError::Other(e.to_string()))?;

            if output.is_none() {
                child.kill().map_err(|e| FixerError::Other(e.to_string()))?;
                return Err(FixerError::Timeout { timeout });
            }
            output.unwrap()
        } else {
            child.wait().map_err(|e| FixerError::Other(e.to_string()))?
        };

        if !status.success() {
            let mut stderr = String::new();
            let mut stderr_buf = std::io::BufReader::new(child.stderr.as_mut().unwrap());
            stderr_buf
                .read_to_string(&mut stderr)
                .map_err(|e| FixerError::Other(format!("Failed to read stderr: {}", e)))?;

            if status.code() == Some(2) {
                return Err(FixerError::NoChanges);
            }

            return Err(FixerError::ScriptFailed {
                path: self.path.to_owned(),
                exit_code: status.code().unwrap(),
                stderr,
            });
        }

        let mut stdout_buf = std::io::BufReader::new(child.stdout.as_mut().unwrap());

        let mut stdout = String::new();
        stdout_buf
            .read_to_string(&mut stdout)
            .map_err(FixerError::Io)?;
        parse_script_fixer_output(&stdout).map_err(FixerError::OutputParseError)
    }
}

pub fn read_desc_file<P: AsRef<std::path::Path>>(
    path: P,
    force_subprocess: bool,
) -> Result<impl Iterator<Item = Box<dyn Fixer>>, Box<dyn std::error::Error>> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);

    let data: serde_yaml::Sequence = serde_yaml::from_reader(reader)?;

    let dirname = path.as_ref().parent().unwrap().to_owned();
    let fixer_iter = data.into_iter().map(move |item| {
        let script = item.get("script").unwrap().as_str().unwrap().to_string();
        let lintian_tags = item
            .get("lintian-tags")
            .map(|tags| {
                Some(
                    tags.as_sequence()?
                        .iter()
                        .filter_map(|tag| Some(tag.as_str()?.to_owned()))
                        .collect::<Vec<_>>(),
                )
            })
            .unwrap_or_default();
        let name = std::path::Path::new(script.as_str())
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let script_path = dirname.join(script.as_str());

        load_fixer(
            name.to_owned(),
            lintian_tags.unwrap_or_default(),
            script_path,
            force_subprocess,
        )
    });

    Ok(fixer_iter)
}

#[cfg(test)]
mod read_desc_file_tests {
    #[test]
    fn test_empty() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("empty.desc");
        std::fs::write(&path, "").unwrap();
        assert!(super::read_desc_file(&path, false)
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn test_single() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("single.desc");
        std::fs::write(
            &path,
            r#"---
- script: foo.sh
  lintian-tags:
  - bar
  - baz
"#,
        )
        .unwrap();
        let script_path = td.path().join("foo.sh");
        std::fs::write(script_path, "#!/bin/sh\n").unwrap();
        let fixer = super::read_desc_file(&path, false).unwrap().next().unwrap();
        assert_eq!(fixer.name(), "foo");
        assert_eq!(fixer.lintian_tags(), vec!["bar", "baz"]);
    }
}

fn load_fixer(
    name: String,
    tags: Vec<String>,
    script_path: std::path::PathBuf,
    force_subprocess: bool,
) -> Box<dyn Fixer> {
    #[cfg(feature = "python")]
    if script_path
        .extension()
        .map(|ext| ext == "py")
        .unwrap_or(false)
        && !force_subprocess
    {
        return Box::new(PythonScriptFixer::new(name, tags, script_path));
    }
    Box::new(ScriptFixer::new(name, tags, script_path))
}

/// Return a list of available lintian fixers.
///
/// # Arguments
///
/// * `fixers_dir` - The directory to search for fixers.
/// * `force_subprocess` - Force the use of a subprocess for all fixers.
pub fn available_lintian_fixers(
    fixers_dir: Option<&std::path::Path>,
    force_subprocess: Option<bool>,
) -> Result<impl Iterator<Item = Box<dyn Fixer>>, Box<dyn std::error::Error>> {
    let system_path = find_fixers_dir();
    let fixers_dir = fixers_dir.unwrap_or_else(|| system_path.as_ref().unwrap().as_path());
    let mut fixers = Vec::new();
    // Scan fixers_dir for .desc files
    for entry in std::fs::read_dir(fixers_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().map(|ext| ext == "desc").unwrap_or(false) {
            let fixer_iter = read_desc_file(&path, force_subprocess.unwrap_or(false))?;
            fixers.extend(fixer_iter);
        }
    }

    Ok(fixers.into_iter())
}

#[derive(Debug, PartialEq, Eq)]
pub struct UnknownFixer(pub String);

impl std::fmt::Display for UnknownFixer {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Unknown fixer: {}", self.0)
    }
}

impl std::error::Error for UnknownFixer {}

/// """Select fixers by name, from a list.
///
/// # Arguments
///
/// * `fixers` - List of Fixer objects
/// * `names` - Set of names to select
/// * `exclude` - Set of names to exclude
pub fn select_fixers(
    fixers: Vec<Box<dyn Fixer>>,
    names: Option<&[&str]>,
    exclude: Option<&[&str]>,
) -> Result<Vec<Box<dyn Fixer>>, UnknownFixer> {
    let mut select_set = names.map(|names| names.iter().cloned().collect::<HashSet<_>>());
    let mut exclude_set = exclude.map(|exclude| exclude.iter().cloned().collect::<HashSet<_>>());
    let mut ret = vec![];
    for f in fixers.into_iter() {
        if let Some(select_set) = select_set.as_mut() {
            if !select_set.remove(f.name().as_str()) {
                if let Some(exclude_set) = exclude_set.as_mut() {
                    exclude_set.remove(f.name().as_str());
                }
                continue;
            }
        }
        if let Some(exclude_set) = exclude_set.as_mut() {
            if exclude_set.remove(f.name().as_str()) {
                continue;
            }
        }
        ret.push(f);
    }
    if let Some(select_set) = select_set.filter(|x| !x.is_empty()) {
        Err(UnknownFixer(select_set.iter().next().unwrap().to_string()))
    } else if let Some(exclude_set) = exclude_set.filter(|x| !x.is_empty()) {
        Err(UnknownFixer(exclude_set.iter().next().unwrap().to_string()))
    } else {
        Ok(ret)
    }
}

#[cfg(test)]
mod select_fixers_tests {
    use super::*;

    #[derive(Debug)]
    struct DummyFixer<'a> {
        name: &'a str,
        tags: Vec<&'a str>,
    }

    impl DummyFixer<'_> {
        fn new<'a>(name: &'a str, tags: &[&'a str]) -> DummyFixer<'a> {
            DummyFixer {
                name,
                tags: tags.to_vec(),
            }
        }
    }

    impl<'a> Fixer for DummyFixer<'a> {
        fn name(&self) -> String {
            self.name.to_string()
        }

        fn path(&self) -> std::path::PathBuf {
            unimplemented!()
        }

        fn lintian_tags(&self) -> Vec<String> {
            self.tags.iter().map(|s| s.to_string()).collect::<Vec<_>>()
        }

        fn run(
            &self,
            _basedir: &std::path::Path,
            _package: &str,
            _current_version: &Version,
            _preferences: &FixerPreferences,
            _timeout: Option<chrono::Duration>,
        ) -> Result<FixerResult, FixerError> {
            unimplemented!()
        }
    }

    #[test]
    fn test_exists() {
        assert_eq!(
            Ok(vec!["dummy1".to_string()]),
            select_fixers(
                vec![
                    Box::new(DummyFixer::new("dummy1", &["some-tag"])),
                    Box::new(DummyFixer::new("dummy2", &["other-tag"])),
                ],
                Some(vec!["dummy1"].as_slice()),
                None
            )
            .map(|m| m.into_iter().map(|f| f.name()).collect::<Vec<_>>())
        );
    }

    #[test]
    fn test_missing() {
        assert!(select_fixers(
            vec![
                Box::new(DummyFixer::new("dummy1", &["some-tag"])),
                Box::new(DummyFixer::new("dummy2", &["other-tag"])),
            ],
            Some(vec!["other"].as_slice()),
            None
        )
        .is_err());
    }

    #[test]
    fn test_exclude_missing() {
        assert!(select_fixers(
            vec![
                Box::new(DummyFixer::new("dummy1", &["some-tag"])),
                Box::new(DummyFixer::new("dummy2", &["other-tag"])),
            ],
            Some(vec!["dummy"].as_slice()),
            Some(vec!["some-other"].as_slice())
        )
        .is_err());
    }

    #[test]
    fn test_exclude() {
        assert_eq!(
            Ok(vec!["dummy1".to_string()]),
            select_fixers(
                vec![
                    Box::new(DummyFixer::new("dummy1", &["some-tag"])),
                    Box::new(DummyFixer::new("dummy2", &["other-tag"])),
                ],
                Some(vec!["dummy1"].as_slice()),
                Some(vec!["dummy2"].as_slice())
            )
            .map(|m| m.into_iter().map(|f| f.name()).collect::<Vec<_>>())
        );
    }
}

pub const DEFAULT_VALUE_LINTIAN_BRUSH_ADDON_ONLY: i32 = 10;
pub const DEFAULT_VALUE_LINTIAN_BRUSH: i32 = 50;
pub const LINTIAN_BRUSH_TAG_VALUES: [(&str, i32); 1] = [("trailing-whitespace", 0)];
pub const DEFAULT_ADDON_FIXERS: &[&str] = &[
    "debian-changelog-line-too-long",
    "trailing-whitespace",
    "out-of-date-standards-version",
    "package-uses-old-debhelper-compat-version",
    "public-upstream-key-not-minimal",
];
pub const LINTIAN_BRUSH_TAG_DEFAULT_VALUE: i32 = 5;

pub fn calculate_value(tags: &[&str]) -> i32 {
    if tags.is_empty() {
        return 0;
    }

    let default_addon_fixers: HashSet<&str> = DEFAULT_ADDON_FIXERS.iter().cloned().collect();
    let tag_set: HashSet<&str> = tags.iter().cloned().collect();

    if tag_set.is_subset(&default_addon_fixers) {
        return DEFAULT_VALUE_LINTIAN_BRUSH_ADDON_ONLY;
    }

    let mut value = DEFAULT_VALUE_LINTIAN_BRUSH;

    for tag in tags {
        if let Some(tag_value) = LINTIAN_BRUSH_TAG_VALUES.iter().find(|(t, _)| t == tag) {
            value += tag_value.1;
        } else {
            value += LINTIAN_BRUSH_TAG_DEFAULT_VALUE;
        }
    }

    value
}

pub fn data_file_path(
    name: &str,
    check: impl Fn(&std::path::Path) -> bool,
) -> Option<std::path::PathBuf> {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path = path.join("..").join(name);
    if check(&path) {
        return Some(path);
    }

    pyo3::prepare_freethreaded_python();
    #[cfg(feature = "python")]
    if let Some(path) = pyo3::Python::with_gil(|py| {
        use pyo3::prelude::*;
        let pkg_resources = py.import_bound("pkg_resources").unwrap();
        if let Ok(path) = pkg_resources.call_method1(
            "resource_filename",
            ("lintian_brush", format!("lintian-brush/{}", name)),
        ) {
            if let Ok(path) = path.extract::<std::path::PathBuf>() {
                if check(path.as_path()) {
                    return Some(path);
                }
            }
        }
        None
    }) {
        return Some(path);
    }

    let base_paths = &["/usr/share/lintian-brush", "/usr/local/share/lintian-brush"];

    for base_path in base_paths {
        let path = std::path::Path::new(base_path).join(name);
        if check(&path) {
            return Some(path);
        }
    }

    None
}

pub fn find_fixers_dir() -> Option<std::path::PathBuf> {
    data_file_path("fixers", |path| path.is_dir())
}

/// Run a lintian fixer on a tree.
///
/// # Arguments
///
///  * `local_tree`: WorkingTree object
///  * `basis_tree`: Tree
///  * `fixer`: Fixer object to apply
///  * `committer`: Optional committer (name and email)
///  * `update_changelog`: Whether to add a new entry to the changelog
///  * `compat_release`: Minimum release that the package should be usable on
///  * `  (e.g. 'stable' or 'unstable')
///  * `minimum_certainty`: How certain the fixer should be
///  * `  about its changes.
///  * `trust_package`: Whether to run code from the package if necessary
///  * `allow_reformatting`: Whether to allow reformatting of changed files
///  * `dirty_tracker`: Optional object that can be used to tell if the tree
///  * `  has been changed.
///  * `subpath`: Path in tree to operate on
///  * `net_access`: Whether to allow accessing external services
///  * `opinionated`: Whether to be opinionated
///  * `diligence`: Level of diligence
///
/// # Returns
///   tuple with set of FixerResult, summary of the changes
pub fn run_lintian_fixer(
    local_tree: &WorkingTree,
    fixer: &dyn Fixer,
    committer: Option<&str>,
    mut update_changelog: impl FnMut() -> bool,
    preferences: &FixerPreferences,
    dirty_tracker: &mut Option<DirtyTreeTracker>,
    subpath: &std::path::Path,
    timestamp: Option<chrono::naive::NaiveDateTime>,
    basis_tree: Option<&dyn Tree>,
    changes_by: Option<&str>,
    timeout: Option<chrono::Duration>,
) -> Result<(FixerResult, String), FixerError> {
    let changes_by = changes_by.unwrap_or("lintian-brush");

    let changelog_path = subpath.join("debian/changelog");

    let r = match local_tree.get_file(changelog_path.as_path()) {
        Ok(f) => f,
        Err(Error::NoSuchFile(_pb)) => {
            return Err(FixerError::NotDebianPackage(
                local_tree.abspath(subpath).unwrap(),
            ));
        }
        Err(e) => return Err(FixerError::Other(e.to_string())),
    };

    let cl = ChangeLog::read(r)?;
    let first_entry = if let Some(entry) = cl.entries().next() {
        entry
    } else {
        return Err(FixerError::InvalidChangelog(
            local_tree.abspath(subpath).unwrap(),
            "No entries in changelog".to_string(),
        ));
    };
    let package = first_entry.package().unwrap();
    let current_version: Version =
        if first_entry.distributions().as_deref().unwrap() == vec!["UNRELEASED"] {
            first_entry.version().unwrap()
        } else {
            let mut version = first_entry.version().unwrap();
            version.increment_debian();
            version
        };

    let mut _bt = None;
    let basis_tree: &dyn Tree = if let Some(basis_tree) = basis_tree {
        basis_tree
    } else {
        _bt = Some(local_tree.basis_tree().unwrap());
        _bt.as_ref().unwrap()
    };

    let (mut result, changes, mut specific_files) = match apply_or_revert(
        local_tree,
        subpath,
        basis_tree,
        dirty_tracker.as_mut(),
        |basedir| {
            log::debug!("Running fixer {:?}", fixer);
            let result = fixer.run(
                basedir,
                package.as_str(),
                &current_version,
                preferences,
                timeout,
            )?;
            if let Some(certainty) = result.certainty {
                if !certainty_sufficient(certainty, preferences.minimum_certainty) {
                    return Err(FixerError::NotCertainEnough(
                        certainty,
                        preferences.minimum_certainty,
                        result.overridden_lintian_issues,
                    ));
                }
            }

            if result.description.is_empty() {
                return Err(FixerError::DescriptionMissing);
            }

            Ok(result)
        },
    ) {
        Ok(r) => r,
        Err(ApplyError::NoChanges(r)) => {
            return Err(FixerError::NoChangesAfterOverrides(
                r.overridden_lintian_issues,
            ));
        }
        Err(ApplyError::BrzError(e)) => {
            return Err(e.into());
        }
        Err(ApplyError::CallbackError(e)) => {
            return Err(e);
        }
    };

    let lines = result.description.split('\n').collect::<Vec<_>>();
    let mut summary = lines[0].to_string();
    let details = lines
        .iter()
        .skip(1)
        .take_while(|l| !l.is_empty())
        .collect::<Vec<_>>();

    // If there are upstream changes in a non-native package, perhaps
    // export them to debian/patches
    if has_non_debian_changes(changes.as_slice(), subpath)
        && current_version.debian_revision.is_some()
    {
        let (patch_name, updated_specific_files) = match upstream_changes_to_patch(
            local_tree,
            basis_tree,
            dirty_tracker.as_mut(),
            subpath,
            &result
                .patch_name
                .as_deref()
                .map_or_else(|| fixer.name(), |n| n.to_string()),
            result.description.as_str(),
            timestamp.map(|t| t.date()),
        ) {
            Ok(r) => r,
            Err(e) => {
                reset_tree_with_dirty_tracker(
                    local_tree,
                    Some(basis_tree),
                    Some(subpath),
                    dirty_tracker.as_mut(),
                )
                .map_err(|e| FixerError::Other(e.to_string()))?;

                return Err(FixerError::FailedPatchManipulation(e.to_string()));
            }
        };

        specific_files = Some(updated_specific_files);

        summary = format!("Add patch {}: {}", patch_name, summary);
    }

    let update_changelog = if debian_analyzer::changelog::only_changes_last_changelog_block(
        local_tree,
        basis_tree,
        changelog_path.as_path(),
        changes.iter(),
    )? {
        // If the script only changed the last entry in the changelog,
        // don't update the changelog
        false
    } else {
        update_changelog()
    };

    if update_changelog {
        let mut entry = vec![summary.as_str()];
        entry.extend(details);

        add_changelog_entry(local_tree, changelog_path.as_path(), entry.as_slice())?;
        if let Some(specific_files) = specific_files.as_mut() {
            specific_files.push(changelog_path);
        }
    }

    let mut description = format!("{}\n", result.description);
    description.push('\n');
    description.push_str(format!("Changes-By: {}\n", changes_by).as_str());
    for tag in result.fixed_lintian_tags() {
        description.push_str(format!("Fixes: lintian: {}\n", tag).as_str());
        description
            .push_str(format!("See-also: https://lintian.debian.org/tags/{}.html\n", tag).as_str());
    }

    let committer = committer.map_or_else(|| get_committer(local_tree), |c| c.to_string());

    let specific_files_ref = specific_files
        .as_ref()
        .map(|fs| fs.iter().map(|p| p.as_path()).collect::<Vec<_>>());

    let revid = local_tree
        .commit(
            description.as_str(),
            Some(false),
            Some(committer.as_str()),
            specific_files_ref.as_deref(),
        )
        .map_err(|e| match e {
            Error::PointlessCommit => FixerError::NoChanges,
            Error::NoWhoami => FixerError::Other("No committer specified".to_string()),
            e => FixerError::Other(e.to_string()),
        })?;
    result.revision_id = Some(revid);

    // TODO(jelmer): Support running sbuild & verify lintian warning is gone?
    Ok((result, summary))
}

#[derive(Debug)]
pub enum OverallError {
    NotDebianPackage(std::path::PathBuf),
    WorkspaceDirty(std::path::PathBuf),
    ChangelogCreate(String),
    InvalidChangelog(std::path::PathBuf, String),
    BrzError(Error),
    IoError(std::io::Error),
    Other(String),
    #[cfg(feature = "python")]
    Python(pyo3::PyErr),
}

#[cfg(feature = "python")]
impl From<pyo3::PyErr> for OverallError {
    fn from(e: pyo3::PyErr) -> Self {
        OverallError::Python(e)
    }
}

impl std::fmt::Display for OverallError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            OverallError::NotDebianPackage(path) => {
                write!(f, "Not a Debian package: {}", path.display())
            }
            OverallError::WorkspaceDirty(path) => {
                write!(f, "Workspace is dirty: {}", path.display())
            }
            OverallError::ChangelogCreate(m) => {
                write!(f, "Failed to create changelog entry: {}", m)
            }
            #[cfg(feature = "python")]
            OverallError::Python(e) => write!(f, "{}", e),
            OverallError::Other(e) => write!(f, "{}", e),
            OverallError::BrzError(e) => write!(f, "{}", e),
            OverallError::IoError(e) => write!(f, "{}", e),
            OverallError::InvalidChangelog(path, e) => {
                write!(f, "Invalid changelog at {}: {}", path.display(), e)
            }
        }
    }
}

impl std::error::Error for OverallError {}

/// Run a set of lintian fixers on a tree.
///
/// # Arguments
///
///  * `tree`: The tree to run the fixers on
///  * `fixers`: A set of Fixer objects
///  * `update_changelog`: Whether to add an entry to the changelog
///  * `verbose`: Whether to be verbose
///  * `committer`: Optional committer (name and email)
///  * `compat_release`: Minimum release that the package should be usable on
///       (e.g. 'sid' or 'stretch')
///  * `minimum_certainty`: How certain the fixer should be about its changes.
///  * `trust_package`: Whether to run code from the package if necessary
///  * `allow_reformatting`: Whether to allow reformatting of changed files
///  * `use_inotify`: Use inotify to watch changes (significantly improves
///       performance). Defaults to None (automatic)
///  * `subpath`: Subpath in the tree in which the package lives
///  * `net_access`: Whether to allow network access
///  * `opinionated`: Whether to be opinionated
///  * `diligence`: Level of diligence
///  * `changes_by`: Name of the person making the changes
///  * `timeout`: Per-fixer timeout
///
/// # Returns:
///   Tuple with two lists:
///     1. list of tuples with (lintian-tag, certainty, description) of fixers
///        that ran
///     2. dictionary mapping fixer names for fixers that failed to run to the
///        error that occurred
pub fn run_lintian_fixers(
    local_tree: &WorkingTree,
    fixers: &[Box<dyn Fixer>],
    mut update_changelog: Option<impl FnMut() -> bool>,
    verbose: bool,
    committer: Option<&str>,
    preferences: &FixerPreferences,
    use_dirty_tracker: Option<bool>,
    subpath: Option<&std::path::Path>,
    changes_by: Option<&str>,
    timeout: Option<chrono::Duration>,
) -> Result<ManyResult, OverallError> {
    let subpath = subpath.unwrap_or_else(|| std::path::Path::new(""));
    let mut basis_tree = local_tree.basis_tree().unwrap();
    check_clean_tree(local_tree, &basis_tree, subpath).map_err(|e| match e {
        Error::WorkspaceDirty(p) => OverallError::WorkspaceDirty(p),
        e => OverallError::Other(e.to_string()),
    })?;

    let mut changelog_behaviour = None;

    // If we don't know whether to update the changelog, then find out *once*
    let mut update_changelog = || {
        if let Some(update_changelog) = update_changelog.as_mut() {
            return update_changelog();
        }
        let debian_path = subpath.join("debian");
        let cb = determine_update_changelog(local_tree, debian_path.as_path());
        changelog_behaviour = Some(cb);
        changelog_behaviour.as_ref().unwrap().update_changelog
    };

    let mut ret = ManyResult::new();
    let pb = ProgressBar::new(fixers.len() as u64);
    #[cfg(test)]
    pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    let mut dirty_tracker = if use_dirty_tracker.unwrap_or(true) {
        Some(DirtyTreeTracker::new_in_subpath(
            local_tree.clone(),
            subpath,
        ))
    } else {
        None
    };
    for fixer in fixers {
        pb.set_message(format!("Running fixer {}", fixer.name()));
        // Get now from chrono
        let start = std::time::SystemTime::now();
        if let Some(dirty_tracker) = dirty_tracker.as_mut() {
            dirty_tracker.mark_clean();
        }
        pb.inc(1);
        match run_lintian_fixer(
            local_tree,
            fixer.as_ref(),
            committer,
            &mut update_changelog,
            preferences,
            &mut dirty_tracker,
            subpath,
            None,
            Some(&basis_tree),
            changes_by,
            timeout,
        ) {
            Err(e) => match e {
                FixerError::NotDebianPackage(path) => {
                    return Err(OverallError::NotDebianPackage(path));
                }
                FixerError::ChangelogCreate(m) => {
                    return Err(OverallError::ChangelogCreate(m));
                }
                FixerError::OutputParseError(ref _e) => {
                    ret.failed_fixers.insert(fixer.name(), e.to_string());
                    if verbose {
                        log::info!("Fixer {} failed to parse output.", fixer.name());
                    }
                    continue;
                }
                FixerError::DescriptionMissing => {
                    ret.failed_fixers.insert(fixer.name(), e.to_string());
                    if verbose {
                        log::info!(
                            "Fixer {} failed because description is missing.",
                            fixer.name()
                        );
                    }
                    continue;
                }
                FixerError::OutputDecodeError(ref _e) => {
                    ret.failed_fixers.insert(fixer.name(), e.to_string());
                    if verbose {
                        log::info!("Fixer {} failed to decode output.", fixer.name());
                    }
                    continue;
                }
                FixerError::FormattingUnpreservable(path) => {
                    ret.formatting_unpreservable
                        .insert(fixer.name(), path.clone());
                    if verbose {
                        log::info!(
                            "Fixer {} was unable to preserve formatting of {}.",
                            fixer.name(),
                            path.display()
                        );
                    }
                    continue;
                }
                FixerError::GeneratedFile(p) => {
                    ret.failed_fixers
                        .insert(fixer.name(), format!("Generated file: {}", p.display()));
                    if verbose {
                        log::info!(
                            "Fixer {} encountered generated file {}",
                            fixer.name(),
                            p.display()
                        );
                    }
                }
                FixerError::ScriptNotFound(ref p) => {
                    ret.failed_fixers.insert(fixer.name(), e.to_string());
                    if verbose {
                        log::info!("Fixer {} ({}) not found.", fixer.name(), p.display());
                    }
                    continue;
                }
                FixerError::ScriptFailed { .. } => {
                    ret.failed_fixers.insert(fixer.name(), e.to_string());
                    if verbose {
                        log::info!("Fixer {} failed to run.", fixer.name());
                        eprintln!("{}", e);
                    }
                    continue;
                }
                FixerError::MemoryError => {
                    ret.failed_fixers.insert(fixer.name(), e.to_string());
                    if verbose {
                        log::info!("Ran out of memory while running fixer {}.", fixer.name());
                    }
                    continue;
                }
                FixerError::BrzError(e) => {
                    return Err(OverallError::BrzError(e));
                }
                FixerError::Io(e) => {
                    return Err(OverallError::IoError(e));
                }
                FixerError::NotCertainEnough(actual_certainty, minimum_certainty, _overrides) => {
                    if verbose {
                        let duration = std::time::SystemTime::now().duration_since(start).unwrap();
                        log::info!(
                    "Fixer {} made changes but not high enough certainty (was {}, needed {}). (took: {:2}s)",
                    fixer.name(),
                    actual_certainty,
                    minimum_certainty.map_or("default".to_string(), |c| c.to_string()),
                    duration.as_secs_f32(),
                );
                    }
                    continue;
                }
                FixerError::FailedPatchManipulation(ref reason) => {
                    if verbose {
                        log::info!("Unable to manipulate upstream patches: {}", reason);
                    }
                    ret.failed_fixers.insert(fixer.name(), e.to_string());
                    continue;
                }
                FixerError::NoChanges => {
                    if verbose {
                        let duration = std::time::SystemTime::now().duration_since(start).unwrap();
                        log::info!(
                            "Fixer {} made no changes. (took: {:2}s)",
                            fixer.name(),
                            duration.as_secs_f32(),
                        );
                    }
                    continue;
                }
                FixerError::NoChangesAfterOverrides(os) => {
                    if verbose {
                        let duration = std::time::SystemTime::now().duration_since(start).unwrap();
                        log::info!(
                            "Fixer {} made no changes. (took: {:2}s)",
                            fixer.name(),
                            duration.as_secs_f32(),
                        );
                    }
                    ret.overridden_lintian_issues.extend(os);
                    continue;
                }
                #[cfg(feature = "python")]
                FixerError::Python(ref ep) => {
                    if verbose {
                        log::info!("Fixer {} failed: {}", fixer.name(), ep);
                    }
                    ret.failed_fixers.insert(fixer.name(), e.to_string());
                    continue;
                }
                FixerError::Other(ref em) => {
                    if verbose {
                        log::info!("Fixer {} failed: {}", fixer.name(), em);
                    }
                    ret.failed_fixers.insert(fixer.name(), e.to_string());
                    continue;
                }
                FixerError::InvalidChangelog(path, reason) => {
                    return Err(OverallError::InvalidChangelog(path, reason));
                }
                FixerError::Timeout { timeout } => {
                    if verbose {
                        log::info!("Fixer {} timed out after {}.", fixer.name(), timeout);
                    }
                    ret.failed_fixers.insert(fixer.name(), e.to_string());
                    continue;
                }
            },
            Ok((result, summary)) => {
                if verbose {
                    let duration = std::time::SystemTime::now().duration_since(start).unwrap();
                    log::info!(
                        "Fixer {} made changes. (took {:2}s)",
                        fixer.name(),
                        duration.as_secs_f32(),
                    );
                }
                ret.success.push((result, summary));
                basis_tree = local_tree.basis_tree().unwrap();
            }
        }
    }
    pb.finish();
    ret.changelog_behaviour = changelog_behaviour;
    Ok(ret)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ManyResult {
    #[serde(rename = "applied")]
    pub success: Vec<(FixerResult, String)>,
    #[serde(rename = "failed")]
    pub failed_fixers: std::collections::HashMap<String, String>,
    pub changelog_behaviour: Option<ChangelogBehaviour>,
    #[serde(skip)]
    pub overridden_lintian_issues: Vec<LintianIssue>,
    #[serde(skip)]
    pub formatting_unpreservable: std::collections::HashMap<String, std::path::PathBuf>,
}

impl ManyResult {
    pub fn tags_count(&self) -> HashMap<&str, u32> {
        self.success
            .iter()
            .fold(HashMap::new(), |mut acc, (r, _summary)| {
                for tag in r.fixed_lintian_tags() {
                    *acc.entry(tag).or_insert(0) += 1;
                }
                acc
            })
    }

    pub fn value(&self) -> i32 {
        let tags = self
            .success
            .iter()
            .flat_map(|(r, _summary)| r.fixed_lintian_tags())
            .collect::<Vec<_>>();
        calculate_value(tags.as_slice())
    }

    /// Return the minimum certainty of any successfully made change.
    pub fn minimum_success_certainty(&self) -> Certainty {
        min_certainty(
            self.success
                .iter()
                .filter_map(|(r, _summary)| r.certainty)
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .unwrap_or(Certainty::Certain)
    }

    pub fn new() -> Self {
        Self {
            success: Vec::new(),
            failed_fixers: std::collections::HashMap::new(),
            changelog_behaviour: None,
            overridden_lintian_issues: Vec::new(),
            formatting_unpreservable: std::collections::HashMap::new(),
        }
    }
}

#[cfg(test)]
mod many_result_tests {
    use super::*;

    #[test]
    fn test_empty() {
        let result = ManyResult::default();
        assert_eq!(Certainty::Certain, result.minimum_success_certainty());
    }

    #[test]
    fn test_no_certainty() {
        let mut result = ManyResult::default();
        result.success.push((
            FixerResult::new(
                "Do bla".to_string(),
                Some(vec!["tag-a".to_string()]),
                None,
                None,
                None,
                vec![],
                None,
            ),
            "summary".to_string(),
        ));
        assert_eq!(Certainty::Certain, result.minimum_success_certainty());
    }

    #[test]
    fn test_possible() {
        let mut result = ManyResult::default();
        result.success.push((
            FixerResult::new(
                "Do bla".to_string(),
                Some(vec!["tag-a".to_string()]),
                Some(Certainty::Possible),
                None,
                None,
                vec![],
                None,
            ),
            "summary".to_string(),
        ));
        result.success.push((
            FixerResult::new(
                "Do bloeh".to_string(),
                Some(vec!["tag-b".to_string()]),
                Some(Certainty::Certain),
                None,
                None,
                vec![],
                None,
            ),
            "summary".to_string(),
        ));
        assert_eq!(Certainty::Possible, result.minimum_success_certainty());
    }
}

fn has_non_debian_changes(changes: &[TreeChange], subpath: &std::path::Path) -> bool {
    let debian_path = subpath.join("debian");
    changes.iter().any(|change| {
        [change.path.0.as_deref(), change.path.1.as_deref()]
            .into_iter()
            .flatten()
            .any(|path| !path.starts_with(&debian_path))
    })
}

#[derive(Debug)]
struct FailedPatchManipulation(String);

impl std::fmt::Display for FailedPatchManipulation {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Failed to manipulate patches: {}", self.0)
    }
}

impl std::error::Error for FailedPatchManipulation {}

fn upstream_changes_to_patch(
    local_tree: &WorkingTree,
    basis_tree: &dyn Tree,
    dirty_tracker: Option<&mut DirtyTreeTracker>,
    subpath: &std::path::Path,
    patch_name: &str,
    description: &str,
    timestamp: Option<chrono::naive::NaiveDate>,
) -> Result<(String, Vec<std::path::PathBuf>), FailedPatchManipulation> {
    use debian_analyzer::patches::{
        move_upstream_changes_to_patch, read_quilt_patches, tree_patches_directory,
    };

    // TODO(jelmer): Apply all patches before generating a diff.

    let patches_directory = tree_patches_directory(local_tree, subpath);
    let quilt_patches =
        read_quilt_patches(local_tree, patches_directory.as_path()).collect::<Vec<_>>();
    if !quilt_patches.is_empty() {
        return Err(FailedPatchManipulation(
            "Creating patch on top of existing quilt patches not supported.".to_string(),
        ));
    }

    log::debug!("Moving upstream changes to patch {}", patch_name);
    let (specific_files, patch_name) = match move_upstream_changes_to_patch(
        local_tree,
        basis_tree,
        subpath,
        patch_name,
        description,
        dirty_tracker,
        timestamp,
    ) {
        Ok(r) => r,
        Err(e) => {
            return Err(FailedPatchManipulation(e.to_string()));
        }
    };

    Ok((patch_name, specific_files))
}

fn note_changelog_policy(policy: bool, msg: &str) {
    lazy_static::lazy_static! {
        static ref CHANGELOG_POLICY_NOTED: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    }
    if let Ok(mut policy_noted) = CHANGELOG_POLICY_NOTED.lock() {
        if !*policy_noted {
            let extra = if policy {
                "Specify --no-update-changelog to override."
            } else {
                "Specify --update-changelog to override."
            };
            log::info!("{} {}", msg, extra);
        }
        *policy_noted = true;
    }
}

pub fn determine_update_changelog(
    local_tree: &WorkingTree,
    debian_path: &std::path::Path,
) -> ChangelogBehaviour {
    let changelog_path = debian_path.join("changelog");

    let cl = match local_tree.get_file(changelog_path.as_path()) {
        Ok(f) => ChangeLog::read(f).unwrap(),

        Err(Error::NoSuchFile(_)) => {
            // If there's no changelog, then there's nothing to update!
            return ChangelogBehaviour {
                update_changelog: false,
                explanation: "No changelog found".to_string(),
            };
        }
        Err(e) => {
            panic!("Error reading changelog: {}", e);
        }
    };

    let behaviour = guess_update_changelog(local_tree, debian_path, Some(cl));

    let behaviour = if let Some(behaviour) = behaviour {
        note_changelog_policy(behaviour.update_changelog, behaviour.explanation.as_str());
        behaviour
    } else {
        // If we can't make an educated guess, assume yes.
        ChangelogBehaviour {
            update_changelog: true,
            explanation: "Assuming changelog should be updated".to_string(),
        }
    };

    behaviour
}

#[cfg(test)]
mod run_lintian_fixers_test {
    use super::*;
    use breezyshim::controldir::ControlDirFormat;
    use breezyshim::tree::{MutableTree, WorkingTree};
    const CHANGELOG_FILE: &str = r#"blah (0.1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"#;

    #[derive(Debug)]
    struct DummyFixer {
        name: String,
        lintian_tags: Vec<String>,
    }

    impl DummyFixer {
        fn new(name: &str, lintian_tags: &[&str]) -> Self {
            Self {
                name: name.to_string(),
                lintian_tags: lintian_tags.iter().map(|t| t.to_string()).collect(),
            }
        }
    }

    impl Fixer for DummyFixer {
        fn name(&self) -> String {
            self.name.clone()
        }

        fn path(&self) -> std::path::PathBuf {
            std::path::PathBuf::from("/dev/null")
        }

        fn lintian_tags(&self) -> Vec<String> {
            self.lintian_tags.clone()
        }

        fn run(
            &self,
            basedir: &std::path::Path,
            package: &str,
            _current_version: &Version,
            _preferences: &FixerPreferences,
            _timeout: Option<chrono::Duration>,
        ) -> Result<FixerResult, FixerError> {
            std::fs::write(basedir.join("debian/control"), "a new line\n").unwrap();
            Ok(FixerResult {
                description: "Fixed some tag.\nExtended description.".to_string(),
                patch_name: None,
                certainty: Some(Certainty::Certain),
                fixed_lintian_issues: vec![LintianIssue {
                    tag: Some("some-tag".to_string()),
                    package: Some(package.to_string()),
                    info: None,
                    package_type: Some(PackageType::Source),
                }],
                overridden_lintian_issues: vec![],
                revision_id: None,
            })
        }
    }

    #[derive(Debug)]
    struct FailingFixer {
        name: String,
        lintian_tags: Vec<String>,
    }

    impl FailingFixer {
        fn new(name: &str, lintian_tags: &[&str]) -> Self {
            Self {
                name: name.to_string(),
                lintian_tags: lintian_tags.iter().map(|t| t.to_string()).collect(),
            }
        }
    }

    impl Fixer for FailingFixer {
        fn name(&self) -> String {
            self.name.clone()
        }

        fn path(&self) -> std::path::PathBuf {
            std::path::PathBuf::from("/dev/null")
        }

        fn lintian_tags(&self) -> Vec<String> {
            self.lintian_tags.clone()
        }

        fn run(
            &self,
            basedir: &std::path::Path,
            _package: &str,
            _current_version: &Version,
            _preferences: &FixerPreferences,
            _timeout: Option<chrono::Duration>,
        ) -> Result<FixerResult, FixerError> {
            std::fs::write(basedir.join("debian/foo"), "blah").unwrap();
            std::fs::write(basedir.join("debian/control"), "foo\n").unwrap();
            Err(FixerError::ScriptFailed {
                stderr: "Not successful".to_string(),
                path: std::path::PathBuf::from("/dev/null"),
                exit_code: 1,
            })
        }
    }

    fn setup() -> (tempfile::TempDir, WorkingTree) {
        let td = tempfile::tempdir().unwrap();
        let tree = breezyshim::controldir::create_standalone_workingtree(
            td.path(),
            &ControlDirFormat::default(),
        )
        .unwrap();
        tree.mkdir(std::path::Path::new("debian")).unwrap();
        std::fs::write(
            td.path().join("debian/control"),
            r#""Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"#,
        )
        .unwrap();
        tree.add(&[std::path::Path::new("debian/control")]).unwrap();
        std::fs::write(td.path().join("debian/changelog"), CHANGELOG_FILE).unwrap();
        tree.add(&[std::path::Path::new("debian/changelog")])
            .unwrap();
        tree.commit("Initial thingy.", None, None, None).unwrap();
        (td, tree)
    }

    #[test]
    fn test_fails() {
        let (td, tree) = setup();
        let lock = tree.lock_write().unwrap();
        let result = run_lintian_fixers(
            &tree,
            &[Box::new(FailingFixer::new("fail", &["some-tag"]))],
            Some(|| false),
            false,
            None,
            &FixerPreferences::default(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        std::mem::drop(lock);
        assert_eq!(0, result.success.len());
        assert_eq!(1, result.failed_fixers.len());
        let fixer = result.failed_fixers.get("fail").unwrap();
        assert!(fixer.contains("Not successful"));

        let lock = tree.lock_read().unwrap();
        assert_eq!(
            Vec::<breezyshim::tree::TreeChange>::new(),
            tree.iter_changes(&tree.basis_tree().unwrap(), None, None, None)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        );
        std::mem::drop(lock);
        std::mem::drop(td);
    }

    #[test]
    fn test_not_debian_tree() {
        let (td, tree) = setup();
        tree.remove(&[(std::path::Path::new("debian/changelog"))])
            .unwrap();
        std::fs::remove_file(td.path().join("debian/changelog")).unwrap();
        tree.commit("not a debian dir", None, None, None).unwrap();
        let lock = tree.lock_write().unwrap();

        assert!(matches!(
            run_lintian_fixers(
                &tree,
                &[Box::new(DummyFixer::new("dummy", &["some-tag"][..]))],
                Some(|| false),
                false,
                None,
                &FixerPreferences::default(),
                None,
                None,
                None,
                None,
            ),
            Err(OverallError::NotDebianPackage(_))
        ));
        std::mem::drop(lock);
        std::mem::drop(td);
    }

    #[test]
    fn test_simple_modify() {
        let (td, tree) = setup();
        let lock = tree.lock_write().unwrap();
        let result = run_lintian_fixers(
            &tree,
            &[Box::new(DummyFixer::new("dummy", &["some-tag"]))],
            Some(|| false),
            false,
            None,
            &FixerPreferences::default(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let revid = tree.last_revision().unwrap();
        std::mem::drop(lock);

        assert_eq!(
            vec![(
                FixerResult::new(
                    "Fixed some tag.\nExtended description.".to_string(),
                    None,
                    Some(Certainty::Certain),
                    None,
                    Some(revid),
                    vec![LintianIssue {
                        tag: Some("some-tag".to_string()),
                        package: Some("blah".to_string()),
                        info: None,
                        package_type: Some(PackageType::Source),
                    }],
                    None,
                ),
                "Fixed some tag.".to_string()
            )],
            result.success,
        );
        assert_eq!(maplit::hashmap! {}, result.failed_fixers);
        assert_eq!(2, tree.branch().revno());
        let lines = tree
            .get_file_lines(std::path::Path::new("debian/control"))
            .unwrap();
        assert_eq!(lines.last().unwrap(), &b"a new line\n".to_vec());
        std::mem::drop(td);
    }
}
