use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
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
    ApplyError, ChangelogError,
};
use debian_changelog::ChangeLog;

pub mod builtin_fixers;
#[macro_use]
pub mod macros;
pub mod fixers;
pub mod licenses;
pub mod lintian_overrides;
pub mod upstream_metadata;
pub mod watch;

// Re-export commonly used types for convenience
pub use debian_analyzer::Certainty;
pub use debversion::Version;
// Re-export inventory for macros
pub use inventory;

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

    /// Check if this issue should be fixed (i.e., it's not overridden)
    pub fn should_fix(&self, base_path: &std::path::Path) -> bool {
        use crate::lintian_overrides;

        for line in lintian_overrides::iter_overrides(base_path) {
            if line.matches(self) {
                return false;
            }
        }

        true
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LintianIssueParseError {
    InvalidPackageType(String),
}

#[cfg(feature = "python")]
impl<'a, 'py> pyo3::FromPyObject<'a, 'py> for LintianIssue {
    type Error = pyo3::PyErr;

    fn extract(ob: pyo3::Borrowed<'a, 'py, pyo3::PyAny>) -> pyo3::PyResult<Self> {
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
            if let Some((first, second)) = before.trim().split_once(' ') {
                // Check if the format is "package source:" or "source package:"
                if second == "source" {
                    // Format: "package source:"
                    package_type = Some(PackageType::Source);
                    package = Some(first.to_string());
                } else if second == "binary" {
                    // Format: "package binary:"
                    package_type = Some(PackageType::Binary);
                    package = Some(first.to_string());
                } else if first == "source" {
                    // Format: "source package:"
                    package_type = Some(PackageType::Source);
                    package = Some(second.to_string());
                } else if first == "binary" {
                    // Format: "binary package:"
                    package_type = Some(PackageType::Binary);
                    package = Some(second.to_string());
                } else {
                    return Err(LintianIssueParseError::InvalidPackageType(format!(
                        "{} {}",
                        first, second
                    )));
                }
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

    /// Create a builder for constructing a FixerResult
    pub fn builder(description: impl Into<String>) -> FixerResultBuilder {
        FixerResultBuilder::new(description)
    }
}

/// Builder for constructing FixerResult instances
#[derive(Debug, Default)]
pub struct FixerResultBuilder {
    description: String,
    certainty: Option<Certainty>,
    patch_name: Option<String>,
    revision_id: Option<RevisionId>,
    fixed_lintian_issues: Vec<LintianIssue>,
    fixed_lintian_tags: Vec<String>,
    overridden_lintian_issues: Vec<LintianIssue>,
}

impl FixerResultBuilder {
    /// Create a new builder with the required description
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            ..Default::default()
        }
    }

    /// Set the certainty level
    pub fn certainty(mut self, certainty: Certainty) -> Self {
        self.certainty = Some(certainty);
        self
    }

    /// Set the patch name
    pub fn patch_name(mut self, patch_name: impl Into<String>) -> Self {
        self.patch_name = Some(patch_name.into());
        self
    }

    /// Set the revision ID
    pub fn revision_id(mut self, revision_id: RevisionId) -> Self {
        self.revision_id = Some(revision_id);
        self
    }

    /// Add a fixed lintian issue
    pub fn fixed_issue(mut self, issue: LintianIssue) -> Self {
        self.fixed_lintian_issues.push(issue);
        self
    }

    /// Add multiple fixed lintian issues
    pub fn fixed_issues(mut self, issues: impl IntoIterator<Item = LintianIssue>) -> Self {
        self.fixed_lintian_issues.extend(issues);
        self
    }

    /// Add a fixed lintian tag (will be converted to LintianIssue)
    #[deprecated = "use fixed_issue instead"]
    pub fn fixed_tag(mut self, tag: impl Into<String>) -> Self {
        self.fixed_lintian_tags.push(tag.into());
        self
    }

    /// Add multiple fixed lintian tags
    #[deprecated = "use fixed_issues instead"]
    pub fn fixed_tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.fixed_lintian_tags
            .extend(tags.into_iter().map(|t| t.into()));
        self
    }

    /// Add an overridden lintian issue
    pub fn overridden_issue(mut self, issue: LintianIssue) -> Self {
        self.overridden_lintian_issues.push(issue);
        self
    }

    /// Add multiple overridden lintian issues
    pub fn overridden_issues(mut self, issues: impl IntoIterator<Item = LintianIssue>) -> Self {
        self.overridden_lintian_issues.extend(issues);
        self
    }

    /// Build the FixerResult
    pub fn build(self) -> FixerResult {
        let mut fixed_lintian_issues = self.fixed_lintian_issues;

        // Convert tags to issues
        fixed_lintian_issues.extend(
            self.fixed_lintian_tags
                .into_iter()
                .map(LintianIssue::just_tag),
        );

        FixerResult {
            description: self.description,
            certainty: self.certainty,
            patch_name: self.patch_name,
            revision_id: self.revision_id,
            fixed_lintian_issues,
            overridden_lintian_issues: self.overridden_lintian_issues,
        }
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
    if let Some(compat_release) = preferences.compat_release.as_ref() {
        env.insert("COMPAT_RELEASE".to_owned(), compat_release.to_owned());
    }
    if let Some(upgrade_release) = preferences.upgrade_release.as_ref() {
        env.insert("UPGRADE_RELEASE".to_owned(), upgrade_release.to_owned());
    }
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

    // Add Python path for subprocess fixers
    let py_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("py");

    if let Ok(existing_pythonpath) = std::env::var("PYTHONPATH") {
        // Prepend our py directory to existing PYTHONPATH
        env.insert(
            "PYTHONPATH".to_owned(),
            format!("{}:{}", py_path.to_string_lossy(), existing_pythonpath),
        );
    } else {
        // Set PYTHONPATH to just our py directory
        env.insert(
            "PYTHONPATH".to_owned(),
            py_path.to_string_lossy().to_string(),
        );
    }

    // Add any extra environment variables from preferences (used in tests)
    if let Some(extra_env) = &preferences.extra_env {
        for (key, value) in extra_env {
            env.insert(key.clone(), value.clone());
        }
    }

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
    pub upgrade_release: Option<String>,
    pub extra_env: Option<std::collections::HashMap<String, String>>,
    pub lintian_data_path: Option<std::path::PathBuf>,
}

/// A fixer script
///
/// The `lintian_tags attribute contains the name of the lintian tags this fixer addresses.
pub trait Fixer: std::fmt::Debug + Sync {
    /// Name of the fixer
    fn name(&self) -> String;

    /// Lintian tags this fixer addresses
    fn lintian_tags(&self) -> Vec<String>;

    /// Enable downcasting to concrete types
    fn as_any(&self) -> &dyn std::any::Any;

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

/// Trait for external fixers that have a file path
pub trait ExternalFixer: Fixer {
    /// Path to the fixer script
    fn path(&self) -> std::path::PathBuf;
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
// PyO3 macros rely on a gil-refs feature that is not available in lintian-brush
#[allow(unexpected_cfgs)]
fn run_inline_python_fixer(
    path: &std::path::Path,
    name: &str,
    code: &str,
    basedir: &std::path::Path,
    env: HashMap<String, String>,
    _timeout: Option<chrono::Duration>,
) -> Result<FixerResult, FixerError> {
    use pyo3::import_exception;
    use pyo3::prelude::*;
    use pyo3::types::PyDict;

    import_exception!(debmutate.reformatting, FormattingUnpreservable);
    import_exception!(debian.changelog, ChangelogCreateError);

    Python::attach(|py| {
        let sys = py.import("sys")?;
        let os = py.import("os")?;
        let io = py.import("io")?;
        let fixer_module = py.import("lintian_brush.fixer")?;

        let old_env = os.getattr("environ")?.unbind();
        let old_stderr = sys.getattr("stderr")?;
        let old_stdout = sys.getattr("stdout")?;

        let temp_stderr = io.call_method0("StringIO")?;
        let temp_stdout = io.call_method0("StringIO")?;

        sys.setattr("stderr", &temp_stderr)?;
        sys.setattr("stdout", &temp_stdout)?;
        os.setattr("environ", env)?;

        let old_cwd = os.call_method0("getcwd").ok();

        os.call_method1("chdir", (basedir,))?;

        let global_vars = PyDict::new(py);
        global_vars.set_item("__file__", path)?;
        global_vars.set_item("__name__", "__main__")?;

        use std::ffi::CString;
        let path_cstr = CString::new(path.to_str().unwrap()).unwrap();
        let name_cstr = CString::new(name).unwrap();
        let code_cstr = CString::new(code).unwrap();
        let script_result = PyModule::from_code(py, &code_cstr, &path_cstr, &name_cstr);

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
                    let traceback = py.import("traceback")?;
                    let traceback_io = io.call_method0("StringIO")?;
                    let kwargs = pyo3::types::PyDict::new(py);
                    kwargs.set_item("file", &traceback_io)?;
                    traceback.call_method(
                        "print_exception",
                        (e.get_type(py), &e, e.traceback(py)),
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
    fn setup() {
        pyo3::Python::attach(|py| {
            use pyo3::prelude::*;
            let sys = py.import("sys").unwrap();
            let path = sys.getattr("path").unwrap();
            let mut path: Vec<String> = path.extract().unwrap();
            let extra_path =
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR").to_string() + "/../py")
                    .canonicalize()
                    .unwrap();
            if !path.contains(&extra_path.to_string_lossy().to_string()) {
                path.insert(0, extra_path.to_string_lossy().to_string());
                sys.setattr("path", path).unwrap();
            }
        });
    }

    #[test]
    fn test_no_changes() {
        setup();
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
        setup();
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
        assert!(
            matches!(
                result,
                Err(super::FixerError::ScriptFailed { exit_code: 1, .. })
            ),
            "Result: {:?}",
            result
        );
        std::mem::drop(td);
    }

    #[test]
    #[ignore]
    fn test_timeout() {
        setup();
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

    fn lintian_tags(&self) -> Vec<String> {
        self.lintian_tags.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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

#[cfg(feature = "python")]
impl ExternalFixer for PythonScriptFixer {
    fn path(&self) -> std::path::PathBuf {
        self.path.clone()
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
    Panic {
        message: String,
        backtrace: Option<std::backtrace::Backtrace>,
    },
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
            debian_analyzer::editor::EditorError::TemplateError(p, _e) => {
                FixerError::GeneratedFile(p)
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

impl From<debian_changelog::ParseError> for FixerError {
    fn from(e: debian_changelog::ParseError) -> Self {
        FixerError::ChangelogCreate(e.to_string())
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
            FixerError::Timeout { timeout } => write!(
                f,
                "Timeout after {}",
                humantime::format_duration(timeout.to_std().unwrap())
            ),
            FixerError::GeneratedFile(p) => write!(f, "Generated file: {}", p.display()),
            FixerError::Panic { message, backtrace } => {
                write!(f, "Panic: {}", message)?;
                if let Some(bt) = backtrace {
                    write!(f, "\nBacktrace:\n{}", bt)?;
                }
                Ok(())
            }
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

    fn lintian_tags(&self) -> Vec<String> {
        self.lintian_tags.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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

        let mut cmd = Command::new(self.path.as_os_str());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.current_dir(basedir);

        for (key, value) in env.iter() {
            cmd.env(key, value);
        }

        // For timeout case, we need to handle it differently
        let output = if let Some(timeout) = timeout {
            use std::io::Read;
            use wait_timeout::ChildExt;

            let mut child = cmd.spawn().map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => FixerError::ScriptNotFound(self.path.clone()),
                _ => FixerError::Other(e.to_string()),
            })?;

            let std_timeout = timeout
                .to_std()
                .map_err(|e| FixerError::Other(e.to_string()))?;

            let status = match child
                .wait_timeout(std_timeout)
                .map_err(|e| FixerError::Other(e.to_string()))?
            {
                Some(status) => status,
                None => {
                    child.kill().map_err(|e| FixerError::Other(e.to_string()))?;
                    return Err(FixerError::Timeout { timeout });
                }
            };

            // Read stdout and stderr after process completes
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(mut stdout_reader) = child.stdout.take() {
                stdout_reader
                    .read_to_end(&mut stdout)
                    .map_err(|e| FixerError::Other(format!("Failed to read stdout: {}", e)))?;
            }
            if let Some(mut stderr_reader) = child.stderr.take() {
                stderr_reader
                    .read_to_end(&mut stderr)
                    .map_err(|e| FixerError::Other(format!("Failed to read stderr: {}", e)))?;
            }

            std::process::Output {
                status,
                stdout,
                stderr,
            }
        } else {
            cmd.output().map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => FixerError::ScriptNotFound(self.path.clone()),
                _ => FixerError::Other(e.to_string()),
            })?
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            if output.status.code() == Some(2) {
                return Err(FixerError::NoChanges);
            }

            if output.status.code().is_none() {
                return Err(FixerError::Other("Script terminated by signal".to_string()));
            }

            return Err(FixerError::ScriptFailed {
                path: self.path.to_owned(),
                exit_code: output.status.code().unwrap(),
                stderr,
            });
        }

        parse_script_fixer_output(&stdout).map_err(FixerError::OutputParseError)
    }
}

impl ExternalFixer for ScriptFixer {
    fn path(&self) -> std::path::PathBuf {
        self.path.clone()
    }
}

#[derive(Debug, serde::Deserialize)]
struct FixerDescEntry {
    script: String,
    #[serde(rename = "lintian-tags")]
    lintian_tags: Option<Vec<String>>,
    #[serde(rename = "force-subprocess")]
    force_subprocess: Option<bool>,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, serde::Deserialize)]
struct FixerDescFile {
    fixers: Vec<FixerDescEntry>,
}

#[derive(Debug)]
pub enum FixerDiscoverError {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    NoFixersDir,
}

impl From<std::io::Error> for FixerDiscoverError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_yaml::Error> for FixerDiscoverError {
    fn from(e: serde_yaml::Error) -> Self {
        Self::Yaml(e)
    }
}

impl std::fmt::Display for FixerDiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FixerDiscoverError::Io(e) => write!(f, "IO error: {}", e),
            FixerDiscoverError::Yaml(e) => write!(f, "YAML error: {}", e),
            FixerDiscoverError::NoFixersDir => write!(f, "No fixers directory found"),
        }
    }
}

impl std::error::Error for FixerDiscoverError {}

pub fn read_all_desc_file<P: AsRef<std::path::Path>>(
    path: P,
    force_subprocess: bool,
) -> Result<impl Iterator<Item = Box<dyn Fixer>>, FixerDiscoverError> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);

    let data: FixerDescFile = serde_yaml::from_reader(reader)?;

    let dirname = path.as_ref().parent().unwrap().to_owned();
    let fixer_iter = data.fixers.into_iter().map(move |item| {
        // Include all fixers, even disabled ones
        let script = item.script;
        let lintian_tags = item.lintian_tags;
        let force_subprocess = item.force_subprocess.unwrap_or(force_subprocess);
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

pub fn read_desc_file<P: AsRef<std::path::Path>>(
    path: P,
    force_subprocess: bool,
) -> Result<impl Iterator<Item = Box<dyn Fixer>>, FixerDiscoverError> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);

    let data: FixerDescFile = serde_yaml::from_reader(reader)?;

    let dirname = path.as_ref().parent().unwrap().to_owned();
    let fixer_iter = data.fixers.into_iter().filter_map(move |item| {
        // Skip disabled fixers
        if !item.enabled {
            return None;
        }
        let script = item.script;
        let lintian_tags = item.lintian_tags;
        let force_subprocess = item.force_subprocess.unwrap_or(force_subprocess);
        let name = std::path::Path::new(script.as_str())
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let script_path = dirname.join(script.as_str());

        Some(load_fixer(
            name.to_owned(),
            lintian_tags.unwrap_or_default(),
            script_path,
            force_subprocess,
        ))
    });

    Ok(fixer_iter)
}

#[cfg(test)]
mod read_desc_file_tests {
    #[test]
    fn test_empty() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("empty.desc");
        std::fs::write(
            &path,
            r#"---
fixers:
"#,
        )
        .unwrap();
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
fixers:
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
    #[cfg_attr(not(feature = "python"), allow(unused_variables))] force_subprocess: bool,
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

/// Return a list of all lintian fixers (including disabled ones).
///
/// # Arguments
///
/// * `fixers_dir` - The directory to search for fixers.
/// * `force_subprocess` - Force the use of a subprocess for all fixers.
pub fn all_lintian_fixers(
    fixers_dir: Option<&std::path::Path>,
    force_subprocess: Option<bool>,
) -> Result<impl Iterator<Item = Box<dyn Fixer>>, FixerDiscoverError> {
    let fixers_dir = if let Some(fixers_dir) = fixers_dir {
        fixers_dir.to_path_buf()
    } else {
        let system_path = find_fixers_dir();
        if let Some(system_path) = system_path {
            system_path
        } else {
            return Err(FixerDiscoverError::NoFixersDir);
        }
    };
    let mut fixers = Vec::new();
    // Add builtin fixers first
    fixers.extend(builtin_fixers::get_builtin_fixers());
    // Scan fixers_dir for .desc files
    for entry in std::fs::read_dir(fixers_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().map(|ext| ext == "desc").unwrap_or(false) {
            let fixer_iter = read_all_desc_file(&path, force_subprocess.unwrap_or(false))?;
            fixers.extend(fixer_iter);
        }
    }
    Ok(fixers.into_iter())
}

/// Return a list of available lintian fixers (enabled ones only).
/// Get available subprocess-based lintian fixers from a directory.
///
/// # Arguments
///
/// * `fixers_dir` - The directory to search for fixers.
/// * `force_subprocess` - Force the use of a subprocess for all fixers.
pub fn available_subprocess_lintian_fixers(
    fixers_dir: Option<&std::path::Path>,
    force_subprocess: Option<bool>,
) -> Result<impl Iterator<Item = Box<dyn Fixer>>, FixerDiscoverError> {
    let fixers_dir = if let Some(fixers_dir) = fixers_dir {
        fixers_dir.to_path_buf()
    } else {
        let system_path = find_fixers_dir();
        if let Some(system_path) = system_path {
            system_path
        } else {
            return Err(FixerDiscoverError::NoFixersDir);
        }
    };
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

/// Get all available lintian fixers (both builtin and subprocess-based).
///
/// # Arguments
///
/// * `fixers_dir` - The directory to search for fixers.
/// * `force_subprocess` - Force the use of a subprocess for all fixers.
pub fn available_lintian_fixers(
    fixers_dir: Option<&std::path::Path>,
    force_subprocess: Option<bool>,
) -> Result<impl Iterator<Item = Box<dyn Fixer>>, FixerDiscoverError> {
    let mut fixers = Vec::new();

    // Add builtin fixers first
    fixers.extend(builtin_fixers::get_builtin_fixers());

    // Add subprocess-based fixers
    fixers.extend(available_subprocess_lintian_fixers(
        fixers_dir,
        force_subprocess,
    )?);

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
    struct DummyFixer {
        name: String,
        tags: Vec<String>,
    }

    impl DummyFixer {
        fn new(name: &str, tags: &[&str]) -> DummyFixer {
            DummyFixer {
                name: name.to_string(),
                tags: tags.iter().map(|s| s.to_string()).collect(),
            }
        }
    }

    impl Fixer for DummyFixer {
        fn name(&self) -> String {
            self.name.clone()
        }

        fn lintian_tags(&self) -> Vec<String> {
            self.tags.clone()
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
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

    #[cfg(feature = "python")]
    {
        pyo3::Python::initialize();
        if let Some(path) = pyo3::Python::attach(|py| {
            use pyo3::prelude::*;
            let pkg_resources = py.import("pkg_resources").unwrap();
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
    local_tree: &breezyshim::workingtree::GenericWorkingTree,
    fixer: &dyn Fixer,
    committer: Option<&str>,
    mut update_changelog: impl FnMut() -> bool,
    preferences: &FixerPreferences,
    dirty_tracker: &mut Option<DirtyTreeTracker>,
    subpath: &std::path::Path,
    timestamp: Option<chrono::naive::NaiveDateTime>,
    basis_tree: Option<&dyn breezyshim::tree::PyTree>,
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
    let first_entry = if let Some(entry) = cl.iter().next() {
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

    let mut _bt: Option<breezyshim::tree::RevisionTree> = None;
    let basis_tree = if let Some(_basis_tree) = basis_tree {
        // For now, we'll use the local tree's basis_tree since converting trait objects is complex
        local_tree.basis_tree().unwrap()
    } else {
        local_tree.basis_tree().unwrap()
    };

    let make_changes = |basedir: &std::path::Path| -> Result<_, FixerError> {
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

        Ok(result)
    };

    let (mut result, changes, mut specific_files) = match apply_or_revert(
        local_tree,
        subpath,
        &basis_tree,
        dirty_tracker.as_mut(),
        make_changes,
    ) {
        Ok(r) => {
            if r.0.description.is_empty() {
                return Err(FixerError::DescriptionMissing);
            }

            r
        }
        Err(ApplyError::NoChanges(r)) => {
            if r.overridden_lintian_issues.is_empty() {
                return Err(FixerError::NoChanges);
            } else {
                return Err(FixerError::NoChangesAfterOverrides(
                    r.overridden_lintian_issues,
                ));
            }
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
            &basis_tree,
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
                    Some(&basis_tree),
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
        &basis_tree,
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

    let mut builder = local_tree
        .build_commit()
        .message(description.as_str())
        .allow_pointless(false)
        .committer(committer.as_str());

    if let Some(specific_files_ref) = specific_files_ref.as_ref() {
        builder = builder.specific_files(specific_files_ref);
    }

    let revid = builder.commit().map_err(|e| match e {
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
    local_tree: &breezyshim::workingtree::GenericWorkingTree,
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
            Clone::clone(local_tree),
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
                FixerError::Panic {
                    ref message,
                    ref backtrace,
                } => {
                    if verbose {
                        log::error!("Fixer {} panicked: {}", fixer.name(), message);
                        if let Some(bt) = backtrace {
                            log::error!("Backtrace:\n{}", bt);
                        }
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

fn upstream_changes_to_patch<T: breezyshim::tree::PyTree>(
    local_tree: &breezyshim::workingtree::GenericWorkingTree,
    basis_tree: &T,
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
    local_tree: &dyn WorkingTree,
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
mod tests {
    use super::*;
    use breezyshim::controldir::{create_standalone_workingtree, ControlDirFormat};
    use breezyshim::repository::Repository;
    use breezyshim::tree::{MutableTree, WorkingTree};
    use breezyshim::workingtree::GenericWorkingTree;
    use breezyshim::Branch;
    use std::path::Path;

    pub const COMMITTER: &str = "Testsuite <lintian-brush@example.com>";

    mod test_run_lintian_fixer {
        use super::*;

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

            fn lintian_tags(&self) -> Vec<String> {
                self.lintian_tags.clone()
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn run(
                &self,
                basedir: &std::path::Path,
                package: &str,
                _current_version: &Version,
                _preferences: &FixerPreferences,
                _timeout: Option<chrono::Duration>,
            ) -> Result<FixerResult, FixerError> {
                let control_path = basedir.join("debian/control");
                let mut control_content = std::fs::read_to_string(&control_path).unwrap();
                control_content.push_str("a new line\n");
                std::fs::write(control_path, control_content).unwrap();
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

            fn lintian_tags(&self) -> Vec<String> {
                self.lintian_tags.clone()
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
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

        fn setup(version: Option<&str>) -> (tempfile::TempDir, GenericWorkingTree) {
            let version = version.unwrap_or("0.1");
            let td = tempfile::tempdir().unwrap();
            let tree =
                create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
            tree.mkdir(std::path::Path::new("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/control"),
                r#"Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"#,
            )
            .unwrap();
            tree.add(&[std::path::Path::new("debian/control")]).unwrap();
            std::fs::write(
                td.path().join("debian/changelog"),
                format!(
                    r#"blah ({}) UNRELEASED; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"#,
                    version
                ),
            )
            .unwrap();
            tree.add(&[std::path::Path::new("debian/changelog")])
                .unwrap();
            tree.build_commit()
                .message("Initial thingy.")
                .committer(COMMITTER)
                .commit()
                .unwrap();
            (td, tree)
        }

        #[test]
        fn test_fails() {
            let (td, tree) = setup(None);
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
            let (td, tree) = setup(None);
            tree.remove(&[(std::path::Path::new("debian/changelog"))])
                .unwrap();
            std::fs::remove_file(td.path().join("debian/changelog")).unwrap();
            tree.build_commit()
                .message("not a debian dir")
                .committer(COMMITTER)
                .commit()
                .unwrap();
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
            let (td, tree) = setup(None);
            let lock = tree.lock_write().unwrap();
            let result = run_lintian_fixers(
                &tree,
                &[Box::new(DummyFixer::new("dummy", &["some-tag"]))],
                Some(|| false),
                false,
                Some(COMMITTER),
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
                result.success,
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
            );
            assert_eq!(maplit::hashmap! {}, result.failed_fixers);
            assert_eq!(2, tree.branch().revno());
            let lines = tree
                .get_file_lines(std::path::Path::new("debian/control"))
                .unwrap();
            assert_eq!(lines.last().unwrap(), &b"a new line\n".to_vec());
            std::mem::drop(td);
        }

        #[test]
        fn test_simple_modify_too_uncertain() {
            let (td, tree) = setup(None);

            #[derive(Debug)]
            struct UncertainFixer {
                name: String,
                lintian_tags: Vec<String>,
            }

            impl UncertainFixer {
                fn new(name: &str, lintian_tags: &[&str]) -> Self {
                    Self {
                        name: name.to_string(),
                        lintian_tags: lintian_tags.iter().map(|t| t.to_string()).collect(),
                    }
                }
            }

            impl Fixer for UncertainFixer {
                fn name(&self) -> String {
                    self.name.clone()
                }

                fn lintian_tags(&self) -> Vec<String> {
                    self.lintian_tags.clone()
                }

                fn as_any(&self) -> &dyn std::any::Any {
                    self
                }

                fn run(
                    &self,
                    basedir: &std::path::Path,
                    _package: &str,
                    _current_version: &Version,
                    _preferences: &FixerPreferences,
                    _timeout: Option<chrono::Duration>,
                ) -> Result<FixerResult, FixerError> {
                    std::fs::write(basedir.join("debian/somefile"), "test").unwrap();
                    Ok(FixerResult {
                        description: "Renamed a file.".to_string(),
                        patch_name: None,
                        certainty: Some(Certainty::Possible),
                        fixed_lintian_issues: vec![],
                        overridden_lintian_issues: vec![],
                        revision_id: None,
                    })
                }
            }

            let lock_write = tree.lock_write().unwrap();

            let result = run_lintian_fixer(
                &tree,
                &UncertainFixer::new("dummy", &["some-tag"]),
                Some(COMMITTER),
                || false,
                &FixerPreferences {
                    minimum_certainty: Some(Certainty::Certain),
                    ..Default::default()
                },
                &mut None,
                Path::new(""),
                None,
                None,
                None,
                None,
            );

            assert!(
                matches!(result, Err(FixerError::NotCertainEnough(..))),
                "{:?}",
                result
            );
            assert_eq!(1, tree.branch().revno());
            std::mem::drop(lock_write);
            std::mem::drop(td);
        }

        #[test]
        fn test_simple_modify_acceptably_uncertain() {
            let (td, tree) = setup(None);

            #[derive(Debug)]
            struct UncertainFixer {
                name: String,
                lintian_tags: Vec<String>,
            }

            impl UncertainFixer {
                fn new(name: &str, lintian_tags: &[&str]) -> Self {
                    Self {
                        name: name.to_string(),
                        lintian_tags: lintian_tags.iter().map(|t| t.to_string()).collect(),
                    }
                }
            }

            impl Fixer for UncertainFixer {
                fn name(&self) -> String {
                    self.name.clone()
                }

                fn lintian_tags(&self) -> Vec<String> {
                    self.lintian_tags.clone()
                }

                fn as_any(&self) -> &dyn std::any::Any {
                    self
                }

                fn run(
                    &self,
                    basedir: &std::path::Path,
                    _package: &str,
                    _current_version: &Version,
                    _preferences: &FixerPreferences,
                    _timeout: Option<chrono::Duration>,
                ) -> Result<FixerResult, FixerError> {
                    std::fs::write(basedir.join("debian/somefile"), "test").unwrap();
                    Ok(FixerResult {
                        description: "Renamed a file.".to_string(),
                        patch_name: None,
                        certainty: Some(Certainty::Possible),
                        fixed_lintian_issues: vec![],
                        overridden_lintian_issues: vec![],
                        revision_id: None,
                    })
                }
            }

            let lock_write = tree.lock_write().unwrap();

            let (_result, summary) = run_lintian_fixer(
                &tree,
                &UncertainFixer::new("dummy", &["some-tag"]),
                Some("Testsuite <lintian-brush@example.com>"),
                || false,
                &FixerPreferences {
                    minimum_certainty: Some(Certainty::Possible),
                    ..Default::default()
                },
                &mut None,
                Path::new(""),
                None,
                None,
                None,
                None,
            )
            .unwrap();

            assert_eq!("Renamed a file.", summary);

            assert_eq!(2, tree.branch().revno());

            std::mem::drop(lock_write);
            std::mem::drop(td);
        }

        #[test]
        fn test_new_file() {
            let (td, tree) = setup(None);

            #[derive(Debug)]
            struct NewFileFixer {
                name: String,
                lintian_tags: Vec<String>,
            }

            impl NewFileFixer {
                fn new(name: &str, lintian_tags: &[&str]) -> Self {
                    Self {
                        name: name.to_string(),
                        lintian_tags: lintian_tags.iter().map(|t| t.to_string()).collect(),
                    }
                }
            }

            impl Fixer for NewFileFixer {
                fn name(&self) -> String {
                    self.name.clone()
                }

                fn lintian_tags(&self) -> Vec<String> {
                    self.lintian_tags.clone()
                }

                fn as_any(&self) -> &dyn std::any::Any {
                    self
                }

                fn run(
                    &self,
                    basedir: &std::path::Path,
                    package: &str,
                    _current_version: &Version,
                    _preferences: &FixerPreferences,
                    _timeout: Option<chrono::Duration>,
                ) -> Result<FixerResult, FixerError> {
                    std::fs::write(basedir.join("debian/somefile"), "test").unwrap();
                    Ok(FixerResult {
                        description: "Created new file.".to_string(),
                        patch_name: None,
                        certainty: None,
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

            let lock_write = tree.lock_write().unwrap();

            let (result, summary) = run_lintian_fixer(
                &tree,
                &NewFileFixer::new("new-file", &["some-tag"]),
                Some(COMMITTER),
                || false,
                &FixerPreferences::default(),
                &mut None,
                Path::new(""),
                None,
                None,
                None,
                None,
            )
            .unwrap();

            assert_eq!("Created new file.", summary);
            assert_eq!(result.certainty, None);
            assert_eq!(result.fixed_lintian_tags(), &["some-tag"]);
            let rev = tree
                .branch()
                .repository()
                .get_revision(&tree.last_revision().unwrap())
                .unwrap();
            assert_eq!(
                rev.message,
                "Created new file.\n\nChanges-By: lintian-brush\nFixes: lintian: some-tag\nSee-also: https://lintian.debian.org/tags/some-tag.html\n"
            );
            assert_eq!(2, tree.branch().revno());
            let basis_tree = tree.branch().basis_tree().unwrap();
            let basis_lock = basis_tree.lock_read().unwrap();
            assert_eq!(
                basis_tree
                    .get_file_text(Path::new("debian/somefile"))
                    .unwrap(),
                b"test"
            );
            std::mem::drop(basis_lock);
            std::mem::drop(lock_write);
            std::mem::drop(td);
        }

        #[test]
        fn test_rename_file() {
            let (td, tree) = setup(None);

            #[derive(Debug)]
            struct RenameFileFixer {
                name: String,
                lintian_tags: Vec<String>,
            }

            impl RenameFileFixer {
                fn new(name: &str, lintian_tags: &[&str]) -> Self {
                    Self {
                        name: name.to_string(),
                        lintian_tags: lintian_tags.iter().map(|t| t.to_string()).collect(),
                    }
                }
            }

            impl Fixer for RenameFileFixer {
                fn name(&self) -> String {
                    self.name.clone()
                }

                fn lintian_tags(&self) -> Vec<String> {
                    self.lintian_tags.clone()
                }

                fn as_any(&self) -> &dyn std::any::Any {
                    self
                }

                fn run(
                    &self,
                    basedir: &std::path::Path,
                    _package: &str,
                    _current_version: &Version,
                    _preferences: &FixerPreferences,
                    _timeout: Option<chrono::Duration>,
                ) -> Result<FixerResult, FixerError> {
                    std::fs::rename(
                        basedir.join("debian/control"),
                        basedir.join("debian/control.blah"),
                    )
                    .unwrap();
                    Ok(FixerResult {
                        description: "Renamed a file.".to_string(),
                        patch_name: None,
                        certainty: None,
                        fixed_lintian_issues: vec![],
                        overridden_lintian_issues: vec![],
                        revision_id: None,
                    })
                }
            }

            let orig_basis_tree = tree.branch().basis_tree().unwrap();
            let lock_write = tree.lock_write().unwrap();
            let (result, summary) = run_lintian_fixer(
                &tree,
                &RenameFileFixer::new("rename", &["some-tag"]),
                Some(COMMITTER),
                || false,
                &FixerPreferences::default(),
                &mut None,
                Path::new(""),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            assert_eq!("Renamed a file.", summary);
            assert_eq!(result.certainty, None);
            assert_eq!(2, tree.branch().revno());
            let basis_tree = tree.branch().basis_tree().unwrap();
            let basis_lock = basis_tree.lock_read().unwrap();
            let orig_basis_tree_lock = orig_basis_tree.lock_read().unwrap();
            assert!(!basis_tree.has_filename(Path::new("debian/control")));
            assert!(basis_tree.has_filename(Path::new("debian/control.blah")));
            assert_ne!(
                orig_basis_tree.get_revision_id(),
                basis_tree.get_revision_id()
            );
            std::mem::drop(orig_basis_tree_lock);
            std::mem::drop(basis_lock);
            std::mem::drop(lock_write);
            std::mem::drop(td);
        }

        #[test]
        fn test_empty_change() {
            let (td, tree) = setup(None);

            #[derive(Debug)]
            struct EmptyFixer {
                name: String,
                lintian_tags: Vec<String>,
            }

            impl EmptyFixer {
                fn new(name: &str, lintian_tags: &[&str]) -> Self {
                    Self {
                        name: name.to_string(),
                        lintian_tags: lintian_tags.iter().map(|t| t.to_string()).collect(),
                    }
                }
            }

            impl Fixer for EmptyFixer {
                fn name(&self) -> String {
                    self.name.clone()
                }

                fn lintian_tags(&self) -> Vec<String> {
                    self.lintian_tags.clone()
                }

                fn as_any(&self) -> &dyn std::any::Any {
                    self
                }

                fn run(
                    &self,
                    _basedir: &std::path::Path,
                    _package: &str,
                    _current_version: &Version,
                    _preferences: &FixerPreferences,
                    _timeout: Option<chrono::Duration>,
                ) -> Result<FixerResult, FixerError> {
                    Ok(FixerResult {
                        description: "I didn't actually change anything.".to_string(),
                        patch_name: None,
                        certainty: None,
                        fixed_lintian_issues: vec![],
                        overridden_lintian_issues: vec![],
                        revision_id: None,
                    })
                }
            }

            let lock_write = tree.lock_write().unwrap();

            let result = run_lintian_fixer(
                &tree,
                &EmptyFixer::new("empty", &["some-tag"]),
                Some(COMMITTER),
                || false,
                &FixerPreferences::default(),
                &mut None,
                Path::new(""),
                None,
                None,
                None,
                None,
            );

            assert!(matches!(result, Err(FixerError::NoChanges)), "{:?}", result);
            assert_eq!(1, tree.branch().revno());

            assert_eq!(
                Vec::<breezyshim::tree::TreeChange>::new(),
                tree.iter_changes(&tree.basis_tree().unwrap(), None, None, None)
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            );

            std::mem::drop(lock_write);

            std::mem::drop(td);
        }

        #[test]
        fn test_upstream_change() {
            let (td, tree) = setup(Some("0.1-1"));

            #[derive(Debug)]
            struct NewFileFixer {
                name: String,
                lintian_tags: Vec<String>,
            }

            impl NewFileFixer {
                fn new(name: &str, lintian_tags: &[&str]) -> Self {
                    Self {
                        name: name.to_string(),
                        lintian_tags: lintian_tags.iter().map(|t| t.to_string()).collect(),
                    }
                }
            }

            impl Fixer for NewFileFixer {
                fn name(&self) -> String {
                    self.name.clone()
                }

                fn lintian_tags(&self) -> Vec<String> {
                    self.lintian_tags.clone()
                }

                fn as_any(&self) -> &dyn std::any::Any {
                    self
                }

                fn run(
                    &self,
                    basedir: &std::path::Path,
                    _package: &str,
                    _current_version: &Version,
                    _preferences: &FixerPreferences,
                    _timeout: Option<chrono::Duration>,
                ) -> Result<FixerResult, FixerError> {
                    std::fs::write(basedir.join("configure.ac"), "AC_INIT(foo, bar)\n").unwrap();
                    Ok(FixerResult {
                        description: "Created new configure.ac.".to_string(),
                        patch_name: Some("add-config".to_string()),
                        certainty: None,
                        fixed_lintian_issues: vec![],
                        overridden_lintian_issues: vec![],
                        revision_id: None,
                    })
                }
            }

            let lock = tree.lock_write().unwrap();

            let (result, summary) = run_lintian_fixer(
                &tree,
                &NewFileFixer::new("add-config", &["add-config"]),
                Some(COMMITTER),
                || false,
                &FixerPreferences::default(),
                &mut None,
                Path::new(""),
                Some(
                    chrono::DateTime::parse_from_rfc3339("2020-09-08T00:36:35Z")
                        .unwrap()
                        .naive_utc(),
                ),
                None,
                None,
                None,
            )
            .unwrap();
            assert_eq!(
                summary,
                "Add patch add-config.patch: Created new configure.ac."
            );
            assert_eq!(result.certainty, None);
            let rev = tree
                .branch()
                .repository()
                .get_revision(&tree.last_revision().unwrap())
                .unwrap();
            assert_eq!(
                rev.message,
                "Created new configure.ac.\n\nChanges-By: lintian-brush\n"
            );
            assert_eq!(2, tree.branch().revno());
            let basis_tree = tree.branch().basis_tree().unwrap();
            let basis_lock = basis_tree.lock_read().unwrap();
            assert_eq!(
                basis_tree
                    .get_file_text(Path::new("debian/patches/series"))
                    .unwrap(),
                b"add-config.patch\n"
            );
            let lines = basis_tree
                .get_file_lines(Path::new("debian/patches/add-config.patch"))
                .unwrap();
            assert_eq!(lines[0], b"Description: Created new configure.ac.\n");
            assert_eq!(lines[1], b"Origin: other\n");
            assert_eq!(lines[2], b"Last-Update: 2020-09-08\n");
            assert_eq!(lines[3], b"---\n");
            assert_eq!(lines[4], b"=== added file 'configure.ac'\n");
            assert_eq!(
                &lines[5][..(b"--- a/configure.ac".len())],
                b"--- a/configure.ac"
            );
            assert_eq!(
                &lines[6][..(b"+++ b/configure.ac".len())],
                b"+++ b/configure.ac"
            );
            assert_eq!(lines[7], b"@@ -0,0 +1,1 @@\n");
            assert_eq!(lines[8], b"+AC_INIT(foo, bar)\n");

            std::mem::drop(basis_lock);
            std::mem::drop(lock);
            std::mem::drop(td);
        }

        #[test]
        fn test_upstream_change_stacked() {
            let (td, tree) = setup(Some("0.1-1"));

            std::fs::create_dir(td.path().join("debian/patches")).unwrap();
            std::fs::write(td.path().join("debian/patches/series"), "foo\n").unwrap();
            std::fs::write(
                td.path().join("debian/patches/foo"),
                r###"--- /dev/null	2020-09-07 13:26:27.546468905 +0000
+++ a	2020-09-08 01:26:25.811742671 +0000
@@ -0,0 +1 @@
+foo
"###,
            )
            .unwrap();
            tree.add(&[
                Path::new("debian/patches"),
                Path::new("debian/patches/series"),
                Path::new("debian/patches/foo"),
            ])
            .unwrap();
            tree.build_commit()
                .committer(COMMITTER)
                .message("Add patches")
                .commit()
                .unwrap();

            #[derive(Debug)]
            struct NewFileFixer {
                name: String,
                lintian_tags: Vec<String>,
            }

            impl NewFileFixer {
                fn new(name: &str, lintian_tags: &[&str]) -> Self {
                    Self {
                        name: name.to_string(),
                        lintian_tags: lintian_tags.iter().map(|t| t.to_string()).collect(),
                    }
                }
            }

            impl Fixer for NewFileFixer {
                fn name(&self) -> String {
                    self.name.clone()
                }

                fn lintian_tags(&self) -> Vec<String> {
                    self.lintian_tags.clone()
                }

                fn as_any(&self) -> &dyn std::any::Any {
                    self
                }

                fn run(
                    &self,
                    basedir: &std::path::Path,
                    _package: &str,
                    _current_version: &Version,
                    _preferences: &FixerPreferences,
                    _timeout: Option<chrono::Duration>,
                ) -> Result<FixerResult, FixerError> {
                    std::fs::write(basedir.join("configure.ac"), "AC_INIT(foo, bar)\n").unwrap();
                    Ok(FixerResult {
                        description: "Created new configure.ac.".to_string(),
                        patch_name: Some("add-config".to_string()),
                        certainty: None,
                        fixed_lintian_issues: vec![],
                        overridden_lintian_issues: vec![],
                        revision_id: None,
                    })
                }
            }

            let lock = tree.lock_write().unwrap();

            let result = run_lintian_fixer(
                &tree,
                &NewFileFixer::new("add-config", &["add-config"]),
                Some(COMMITTER),
                || false,
                &FixerPreferences::default(),
                &mut None,
                Path::new(""),
                Some(
                    chrono::DateTime::parse_from_rfc3339("2020-09-08T00:36:35Z")
                        .unwrap()
                        .naive_utc(),
                ),
                None,
                None,
                None,
            );

            std::mem::drop(lock);

            assert!(matches!(
                result,
                Err(FixerError::FailedPatchManipulation(..))
            ));
            std::mem::drop(td);
        }

        fn make_package_tree(path: &Path, format: &str) -> GenericWorkingTree {
            let tree = create_standalone_workingtree(path, format).unwrap();
            std::fs::create_dir(path.join("debian")).unwrap();
            std::fs::write(
                path.join("debian/control"),
                r#""Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"#,
            )
            .unwrap();
            std::fs::write(
                path.join("debian/changelog"),
                r#"blah (0.1-1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"#,
            )
            .unwrap();
            tree.add(&[
                Path::new("debian"),
                Path::new("debian/changelog"),
                Path::new("debian/control"),
            ])
            .unwrap();
            tree.build_commit()
                .committer(COMMITTER)
                .message("Initial thingy.")
                .commit()
                .unwrap();
            tree
        }

        fn make_change(tree: &GenericWorkingTree, committer: Option<&str>) {
            let lock = tree.lock_write().unwrap();

            let (result, summary) = run_lintian_fixer(
                tree,
                &DummyFixer::new("dummy", &["some-tag"]),
                committer,
                || false,
                &FixerPreferences::default(),
                &mut None,
                Path::new(""),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            assert_eq!(summary, "Fixed some tag.");
            assert_eq!(vec!["some-tag"], result.fixed_lintian_tags());
            assert_eq!(Some(Certainty::Certain), result.certainty);
            assert_eq!(2, tree.branch().revno());
            let lines = tree.get_file_lines(Path::new("debian/control")).unwrap();
            assert_eq!(lines.last().unwrap(), b"a new line\n");
            std::mem::drop(lock);
        }

        #[test]
        fn test_honors_tree_committer_specified() {
            let td = tempfile::tempdir().unwrap();
            let tree = make_package_tree(td.path(), "git");

            make_change(&tree, Some("Jane Example <jane@example.com>"));

            let rev = tree
                .branch()
                .repository()
                .get_revision(&tree.branch().last_revision())
                .unwrap();
            assert_eq!(rev.committer, "Jane Example <jane@example.com>");
        }

        #[test]
        fn test_honors_tree_committer_config() {
            let td = tempfile::tempdir().unwrap();
            let tree = make_package_tree(td.path(), "git");
            std::fs::write(
                td.path().join(".git/config"),
                r###"
[user]
  email = jane@example.com
  name = Jane Example
"###,
            )
            .unwrap();

            make_change(&tree, None);

            let rev = tree
                .branch()
                .repository()
                .get_revision(&tree.branch().last_revision())
                .unwrap();
            assert_eq!(rev.committer, "Jane Example <jane@example.com>");
        }
    }

    #[test]
    fn test_find_shell_scripts() {
        let td = tempfile::tempdir().unwrap();

        let fixers = td.path().join("fixers");
        std::fs::create_dir(&fixers).unwrap();

        std::fs::create_dir(fixers.join("anotherdir")).unwrap();
        std::fs::write(fixers.join("foo.sh"), "echo 'hello'").unwrap();
        std::fs::write(fixers.join("bar.sh"), "echo 'hello'").unwrap();
        std::fs::write(fixers.join("i-fix-aanother-tag.py"), "print('hello')").unwrap();
        std::fs::write(fixers.join(".hidden"), "echo 'hello'").unwrap();
        std::fs::write(fixers.join("backup-file.sh~"), "echo 'hello'").unwrap();
        std::fs::write(fixers.join("no-extension"), "echo 'hello'").unwrap();
        std::fs::write(
            fixers.join("index.desc"),
            r###"

fixers:
- script: foo.sh
  lintian-tags:
   - i-fix-a-tag

- script: bar.sh
  lintian-tags:
   - i-fix-another-tag
   - no-extension
"###,
        )
        .unwrap();

        let fixers = available_subprocess_lintian_fixers(Some(&fixers), Some(false))
            .unwrap()
            .collect::<Vec<_>>();
        assert_eq!(2, fixers.len());
        assert_eq!(fixers[0].name(), "foo");
        assert_eq!(fixers[1].name(), "bar");
    }

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
}

#[cfg(test)]
mod fixer_tests;

#[cfg(test)]
mod script_fixer_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn test_script_terminated_by_signal() {
        // Create a test script that will be killed
        let td = tempfile::tempdir().unwrap();
        let script_path = td.path().join("test_script.sh");

        // Write a script that will be killed immediately by calling kill on itself
        std::fs::write(&script_path, "#!/bin/bash\nkill -TERM $$\n").unwrap();
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Create a script fixer
        let fixer = ScriptFixer::new(
            "test_fixer".to_string(),
            vec!["test-tag".to_string()],
            script_path.clone(),
        );

        // Create a test working directory with debian/control
        let work_dir = td.path().join("work");
        std::fs::create_dir_all(&work_dir).unwrap();
        std::fs::create_dir_all(work_dir.join("debian")).unwrap();
        std::fs::write(work_dir.join("debian/control"), "Source: test\n").unwrap();

        let version: Version = "1.0-1".parse().unwrap();
        let preferences = FixerPreferences::default();
        let result = fixer.run(&work_dir, "test", &version, &preferences, None);

        // The result should be an error about script being terminated by signal
        match result {
            Err(FixerError::Other(msg)) => {
                assert_eq!(msg, "Script terminated by signal");
            }
            other => panic!(
                "Expected FixerError::Other with signal message, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_script_exit_code_2() {
        // Test that exit code 2 returns NoChanges error
        let td = tempfile::tempdir().unwrap();
        let script_path = td.path().join("test_script.sh");

        // Write a script that exits with code 2
        std::fs::write(&script_path, "#!/bin/bash\nexit 2\n").unwrap();
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let fixer = ScriptFixer::new(
            "test_fixer".to_string(),
            vec!["test-tag".to_string()],
            script_path.clone(),
        );

        let work_dir = td.path().join("work");
        std::fs::create_dir_all(&work_dir).unwrap();
        std::fs::create_dir_all(work_dir.join("debian")).unwrap();
        std::fs::write(work_dir.join("debian/control"), "Source: test\n").unwrap();

        let version: Version = "1.0-1".parse().unwrap();
        let preferences = FixerPreferences::default();
        let result = fixer.run(&work_dir, "test", &version, &preferences, None);

        match result {
            Err(FixerError::NoChanges) => {}
            other => panic!("Expected FixerError::NoChanges, got: {:?}", other),
        }
    }

    #[test]
    fn test_script_success() {
        // Test successful script execution
        let td = tempfile::tempdir().unwrap();
        let script_path = td.path().join("test_script.sh");

        // Write a script that succeeds and outputs valid fixer result
        std::fs::write(
            &script_path,
            "#!/bin/bash\necho 'Fixed: test issue'\necho 'Fixed-Lintian-Tags: test-tag'\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let fixer = ScriptFixer::new(
            "test_fixer".to_string(),
            vec!["test-tag".to_string()],
            script_path.clone(),
        );

        let work_dir = td.path().join("work");
        std::fs::create_dir_all(&work_dir).unwrap();
        std::fs::create_dir_all(work_dir.join("debian")).unwrap();
        std::fs::write(work_dir.join("debian/control"), "Source: test\n").unwrap();

        let version: Version = "1.0-1".parse().unwrap();
        let preferences = FixerPreferences::default();
        let result = fixer.run(&work_dir, "test", &version, &preferences, None);

        match result {
            Ok(fixer_result) => {
                assert_eq!(fixer_result.description, "Fixed: test issue");
                assert_eq!(fixer_result.fixed_lintian_tags(), vec!["test-tag"]);
            }
            Err(e) => panic!("Expected success, got error: {:?}", e),
        }
    }

    #[test]
    fn test_parse_script_fixer_output() {
        // Test basic parsing
        let output = "Fixed: test issue\nFixed-Lintian-Tags: tag1, tag2\n";
        let result = parse_script_fixer_output(output).unwrap();
        assert_eq!(result.description, "Fixed: test issue");
        assert_eq!(result.fixed_lintian_tags(), vec!["tag1", "tag2"]);

        // Test with certainty
        let output = "Fixed: test issue\nCertainty: possible\n";
        let result = parse_script_fixer_output(output).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Possible));

        // Test with patch header
        let output = "Fixed: test issue\nPatch-Name: fix.patch\n";
        let result = parse_script_fixer_output(output).unwrap();
        assert_eq!(result.patch_name, Some("fix.patch".to_string()));
    }
}

#[cfg(test)]
mod fixer_result_builder_tests {
    use super::*;

    #[test]
    fn test_fixer_result_builder_basic() {
        let result = FixerResult::builder("Test fix").build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.certainty, None);
        assert_eq!(result.patch_name, None);
        assert_eq!(result.revision_id, None);
        assert_eq!(result.fixed_lintian_issues.len(), 0);
        assert_eq!(result.overridden_lintian_issues.len(), 0);
    }

    #[test]
    fn test_fixer_result_builder_with_certainty() {
        let result = FixerResult::builder("Test fix")
            .certainty(Certainty::Confident)
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.certainty, Some(Certainty::Confident));
    }

    #[test]
    fn test_fixer_result_builder_with_patch_name() {
        let result = FixerResult::builder("Test fix")
            .patch_name("test.patch")
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.patch_name, Some("test.patch".to_string()));
    }

    #[test]
    fn test_fixer_result_builder_with_fixed_tags() {
        let result = FixerResult::builder("Test fix")
            .fixed_tag("tag1")
            .fixed_tag("tag2")
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.fixed_lintian_tags(), vec!["tag1", "tag2"]);
    }

    #[test]
    fn test_fixer_result_builder_with_fixed_tags_batch() {
        let result = FixerResult::builder("Test fix")
            .fixed_tags(["tag1", "tag2", "tag3"])
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.fixed_lintian_tags(), vec!["tag1", "tag2", "tag3"]);
    }

    #[test]
    fn test_fixer_result_builder_with_fixed_issues() {
        let issue1 = LintianIssue::just_tag("tag1".to_string());
        let issue2 = LintianIssue::just_tag("tag2".to_string());

        let result = FixerResult::builder("Test fix")
            .fixed_issue(issue1)
            .fixed_issue(issue2)
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.fixed_lintian_tags(), vec!["tag1", "tag2"]);
    }

    #[test]
    fn test_fixer_result_builder_with_fixed_issues_batch() {
        let issues = vec![
            LintianIssue::just_tag("tag1".to_string()),
            LintianIssue::just_tag("tag2".to_string()),
        ];

        let result = FixerResult::builder("Test fix")
            .fixed_issues(issues)
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.fixed_lintian_tags(), vec!["tag1", "tag2"]);
    }

    #[test]
    fn test_fixer_result_builder_with_overridden_issues() {
        let issue = LintianIssue::just_tag("overridden-tag".to_string());

        let result = FixerResult::builder("Test fix")
            .overridden_issue(issue)
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.overridden_lintian_issues.len(), 1);
        assert_eq!(
            result.overridden_lintian_issues[0].tag,
            Some("overridden-tag".to_string())
        );
    }

    #[test]
    fn test_fixer_result_builder_with_overridden_issues_batch() {
        let issues = vec![
            LintianIssue::just_tag("tag1".to_string()),
            LintianIssue::just_tag("tag2".to_string()),
        ];

        let result = FixerResult::builder("Test fix")
            .overridden_issues(issues)
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.overridden_lintian_issues.len(), 2);
    }

    #[test]
    fn test_fixer_result_builder_chain_all() {
        let revision_id = breezyshim::RevisionId::null(); // Use null for testing

        let result = FixerResult::builder("Test fix")
            .certainty(Certainty::Certain)
            .patch_name("comprehensive.patch")
            .revision_id(revision_id.clone())
            .fixed_tag("fixed-tag")
            .overridden_issue(LintianIssue::just_tag("overridden-tag".to_string()))
            .build();

        assert_eq!(result.description, "Test fix");
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(result.patch_name, Some("comprehensive.patch".to_string()));
        assert_eq!(result.revision_id, Some(revision_id));
        assert_eq!(result.fixed_lintian_tags(), vec!["fixed-tag"]);
        assert_eq!(result.overridden_lintian_issues.len(), 1);
    }

    #[test]
    fn test_fixer_result_builder_mixed_tags_and_issues() {
        let issue = LintianIssue::just_tag("issue-tag".to_string());

        let result = FixerResult::builder("Test fix")
            .fixed_tag("tag1")
            .fixed_issue(issue)
            .fixed_tag("tag2")
            .build();

        let tags = result.fixed_lintian_tags();
        assert_eq!(tags.len(), 3);
        assert!(tags.contains(&"tag1"));
        assert!(tags.contains(&"tag2"));
        assert!(tags.contains(&"issue-tag"));
    }
}
