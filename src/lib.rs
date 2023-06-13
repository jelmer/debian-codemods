use crate::breezyshim::RevisionId;
use std::fs::File;
use std::io::{BufReader, Read};
use std::process::Command;
use std::str::FromStr;

mod breezyshim;

#[derive(Clone, PartialEq, Eq, Debug, Default, PartialOrd, Ord)]
pub enum Certainty {
    Possible,
    Likely,
    Confident,
    #[default]
    Certain,
}

impl FromStr for Certainty {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "certain" => Ok(Certainty::Certain),
            "confident" => Ok(Certainty::Confident),
            "likely" => Ok(Certainty::Likely),
            "possible" => Ok(Certainty::Possible),
            _ => Err(format!("Invalid certainty: {}", value)),
        }
    }
}

impl ToString for Certainty {
    fn to_string(&self) -> String {
        match self {
            Certainty::Certain => "certain".to_string(),
            Certainty::Confident => "confident".to_string(),
            Certainty::Likely => "likely".to_string(),
            Certainty::Possible => "possible".to_string(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PackageType {
    Source,
    Binary,
}

impl ToString for PackageType {
    fn to_string(&self) -> String {
        match self {
            PackageType::Source => "source".to_string(),
            PackageType::Binary => "binary".to_string(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
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

#[derive(Clone, PartialEq, Eq, Debug)]
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
            overridden_lintian_issues: overridden_lintian_issues.unwrap_or(vec![]),
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
    current_version: &str,
    compat_release: &str,
    minimum_certainty: Certainty,
    trust_package: bool,
    allow_reformatting: bool,
    net_access: bool,
    opinionated: bool,
    diligence: i32,
) -> std::collections::HashMap<String, String> {
    let mut env = std::env::vars().collect::<std::collections::HashMap<_, _>>();
    env.insert("DEB_SOURCE".to_owned(), package.to_owned());
    env.insert("CURRENT_VERSION".to_owned(), current_version.to_string());
    env.insert("COMPAT_RELEASE".to_owned(), compat_release.to_owned());
    env.insert(
        "MINIMUM_CERTAINTY".to_owned(),
        minimum_certainty.to_string(),
    );
    env.insert("TRUST_PACKAGE".to_owned(), trust_package.to_string());
    env.insert(
        "REFORMATTING".to_owned(),
        if allow_reformatting {
            "allow"
        } else {
            "disallow"
        }
        .to_owned(),
    );
    env.insert(
        "NET_ACCESS".to_owned(),
        if net_access { "allow" } else { "disallow" }.to_owned(),
    );
    env.insert(
        "OPINIONATED".to_owned(),
        if opinionated { "yes" } else { "no" }.to_owned(),
    );
    env.insert("DILIGENCE".to_owned(), diligence.to_string());
    env
}

/// A fixer script
///
/// The `lintian_tags attribute contains the name of the lintian tags this fixer addresses.
pub trait Fixer {
    fn name(&self) -> &str;

    fn path(&self) -> &std::path::Path;

    fn lintian_tags(&self) -> Vec<&str>;

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
    ///
    /// # Returns
    ///
    ///  A FixerResult object
    fn run(
        &self,
        basedir: &std::path::Path,
        package: &str,
        current_version: &str,
        compat_release: &str,
        minimum_certainty: Option<Certainty>,
        trust_package: Option<bool>,
        allow_reformatting: Option<bool>,
        net_access: Option<bool>,
        opinionated: Option<bool>,
        diligence: Option<i32>,
    ) -> Result<FixerResult, FixerError>;
}

/// A fixer that is implemented as a Python script.
///
/// This gets used just for Python scripts, and significantly speeds things up because it prevents
/// starting a new Python interpreter for every fixer.
#[cfg(feature = "python")]
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
impl Fixer for PythonScriptFixer {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn path(&self) -> &std::path::Path {
        self.path.as_path()
    }

    fn lintian_tags(&self) -> Vec<&str> {
        self.lintian_tags
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
    }

    fn run(
        &self,
        basedir: &std::path::Path,
        package: &str,
        current_version: &str,
        compat_release: &str,
        minimum_certainty: Option<Certainty>,
        trust_package: Option<bool>,
        allow_reformatting: Option<bool>,
        net_access: Option<bool>,
        opinionated: Option<bool>,
        diligence: Option<i32>,
    ) -> Result<FixerResult, FixerError> {
        let env = determine_env(
            package,
            current_version,
            compat_release,
            minimum_certainty.unwrap_or(Certainty::default()),
            trust_package.unwrap_or(false),
            allow_reformatting.unwrap_or(false),
            net_access.unwrap_or(true),
            opinionated.unwrap_or(false),
            diligence.unwrap_or(0),
        );

        use pyo3::import_exception;
        use pyo3::prelude::*;
        use pyo3::types::PyDict;

        import_exception!(debmutate.reformatting, FormattingUnpreservable);

        Python::with_gil(|py| {
            let sys = py.import("sys")?;
            let os = py.import("os")?;
            let io = py.import("io")?;
            let fixer_module = py.import("lintian_brush.fixer")?;

            let old_env = os.getattr("environ")?.into_py(py);
            let old_stderr = sys.getattr("stderr")?;
            let old_stdout = sys.getattr("stdout")?;

            sys.setattr("stderr", io.call_method0("StringIO")?)?;
            sys.setattr("stdout", io.call_method0("StringIO")?)?;
            os.setattr("environ", env)?;

            let old_cwd = match os.call_method0("getcwd") {
                Ok(cwd) => Some(cwd),
                Err(_) => None,
            };

            os.call_method1("chdir", (basedir,))?;

            let global_vars = PyDict::new(py);
            global_vars.set_item("__file__", &self.path)?;
            global_vars.set_item("__name__", "__main__")?;

            let code = std::fs::read_to_string(&self.path)
                .map_err(|e| FixerError::Other(format!("Failed to read script: {}", e)))?;

            let script_result = PyModule::from_code(
                py,
                code.as_str(),
                self.path.to_str().unwrap(),
                self.name.as_str(),
            );

            let stdout = sys
                .getattr("stdout")
                .unwrap()
                .call_method0("getvalue")
                .unwrap()
                .extract::<String>()
                .unwrap();

            let mut stderr = sys
                .getattr("stderr")
                .unwrap()
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
                        return Err(FixerError::FormattingUnpreservable);
                    } else if e.is_instance_of::<pyo3::exceptions::PySystemExit>(py) {
                        retcode = e.value(py).getattr("code")?.extract()?;
                        description = stdout;
                    } else {
                        use pyo3::types::IntoPyDict;
                        let traceback = py.import("traceback")?;
                        let traceback_io = io.call_method0("StringIO")?;
                        let kwargs = [("file", traceback_io)].into_py_dict(py);
                        traceback.call_method(
                            "print_exception",
                            (e.get_type(py), &e, e.traceback(py)),
                            Some(kwargs),
                        )?;
                        let traceback_str =
                            traceback_io.call_method0("getvalue")?.extract::<String>()?;
                        stderr = format!("{}\n{}", stderr, traceback_str);
                        return Err(FixerError::ScriptFailed {
                            path: self.path.clone(),
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
                    path: self.path.clone(),
                    exit_code: retcode,
                    stderr,
                })
            } else {
                Ok(parse_script_fixer_output(&description)?)
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixerError {
    NoChanges,
    ScriptNotFound(std::path::PathBuf),
    OutputParseError(OutputParseError),
    OutputDecodeError(std::string::FromUtf8Error),
    ScriptFailed {
        path: std::path::PathBuf,
        exit_code: i32,
        stderr: String,
    },
    FormattingUnpreservable,
    Other(String),
}

impl From<OutputParseError> for FixerError {
    fn from(e: OutputParseError) -> Self {
        FixerError::OutputParseError(e)
    }
}

#[cfg(feature = "python")]
impl From<pyo3::PyErr> for FixerError {
    fn from(e: pyo3::PyErr) -> Self {
        FixerError::Other(e.to_string())
    }
}

impl std::fmt::Display for FixerError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FixerError::NoChanges => write!(f, "No changes"),
            FixerError::OutputParseError(e) => write!(f, "Output parse error: {}", e),
            FixerError::OutputDecodeError(e) => write!(f, "Output decode error: {}", e),
            FixerError::ScriptNotFound(p) => write!(f, "Command not found: {}", p.display()),
            FixerError::FormattingUnpreservable => write!(f, "Formatting unpreservable"),
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
        }
    }
}

impl std::error::Error for FixerError {}

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
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn path(&self) -> &std::path::Path {
        self.path.as_path()
    }

    fn lintian_tags(&self) -> Vec<&str> {
        self.lintian_tags
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
    }

    fn run(
        &self,
        basedir: &std::path::Path,
        package: &str,
        current_version: &str,
        compat_release: &str,
        minimum_certainty: Option<Certainty>,
        trust_package: Option<bool>,
        allow_reformatting: Option<bool>,
        net_access: Option<bool>,
        opinionated: Option<bool>,
        diligence: Option<i32>,
    ) -> Result<FixerResult, FixerError> {
        let env = determine_env(
            package,
            current_version,
            compat_release,
            minimum_certainty.unwrap_or(Certainty::default()),
            trust_package.unwrap_or(false),
            allow_reformatting.unwrap_or(false),
            net_access.unwrap_or(true),
            opinionated.unwrap_or(false),
            diligence.unwrap_or(0),
        );

        let mut cmd = Command::new(self.path.as_os_str());
        cmd.current_dir(basedir);

        for (key, value) in env.iter() {
            cmd.env(key, value);
        }

        let output = cmd.output().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => FixerError::ScriptNotFound(self.path.clone()),
            _ => FixerError::Other(e.to_string()),
        })?;

        if !output.status.success() {
            let mut stderr = String::new();
            let mut stderr_buf = std::io::BufReader::new(&output.stderr[..]);
            stderr_buf
                .read_to_string(&mut stderr)
                .map_err(|e| FixerError::Other(format!("Failed to read stderr: {}", e)))?;

            if output.status.code() == Some(2) {
                return Err(FixerError::NoChanges);
            }

            return Err(FixerError::ScriptFailed {
                path: self.path.to_owned(),
                exit_code: output.status.code().unwrap(),
                stderr,
            });
        }

        let stdout = String::from_utf8(output.stdout).map_err(FixerError::OutputDecodeError)?;
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
    fixers_dir: &std::path::Path,
    force_subprocess: Option<bool>,
) -> Result<impl Iterator<Item = Box<dyn Fixer>>, Box<dyn std::error::Error>> {
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

/// Check if the actual certainty is sufficient.
///
/// # Arguments
///
/// * `actual_certainty` - Actual certainty with which changes were made
/// * `minimum_certainty` - Minimum certainty to keep changes
///
/// # Returns
///
/// * `bool` - Whether the actual certainty is sufficient
pub fn certainty_sufficient(
    actual_certainty: Certainty,
    minimum_certainty: Option<Certainty>,
) -> bool {
    if let Some(minimum_certainty) = minimum_certainty {
        actual_certainty >= minimum_certainty
    } else {
        true
    }
}

pub fn min_certainty(certainties: &[Certainty]) -> Option<Certainty> {
    certainties.iter().min().cloned()
}
