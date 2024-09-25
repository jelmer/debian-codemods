use breezyshim::dirty_tracker::DirtyTreeTracker;
use breezyshim::error::Error;
use breezyshim::tree::{Tree, TreeChange, WorkingTree};
use breezyshim::workspace::reset_tree_with_dirty_tracker;
#[cfg(feature = "python")]
use pyo3::prelude::*;

pub mod abstract_control;
pub mod benfile;
pub mod changelog;
pub mod config;
pub mod control;
pub mod debcargo;
pub mod debcommit;
pub mod debhelper;
pub mod detect_gbp_dch;
pub mod editor;
pub mod lintian;
pub mod maintscripts;
pub mod patches;
pub mod publish;
pub mod relations;
pub mod release_info;
pub mod rules;
pub mod salsa;
pub mod svp;
pub mod transition;
#[cfg(feature = "udd")]
pub mod udd;
pub mod vcs;
pub mod vendor;
pub mod versions;
#[cfg(feature = "udd")]
pub mod wnpp;

// TODO(jelmer): Import this from ognibuild
pub const DEFAULT_BUILDER: &str = "sbuild --no-clean-source";

#[derive(Debug)]
pub enum ApplyError<R, E> {
    /// Error from the callback
    CallbackError(E),
    /// Error from the tree
    BrzError(Error),
    /// No changes made
    NoChanges(R),
}

impl<R, E> From<Error> for ApplyError<R, E> {
    fn from(e: Error) -> Self {
        ApplyError::BrzError(e)
    }
}

/// Apply a change in a clean tree.
///
/// This will either run a callback in a tree, or if the callback fails,
/// revert the tree to the original state.
///
/// The original tree should be clean; you can run check_clean_tree() to
/// verify this.
///
/// # Arguments
/// * `local_tree` - Local tree
/// * `subpath` - Subpath to apply changes to
/// * `basis_tree` - Basis tree to reset to
/// * `dirty_tracker` - Dirty tracker
/// * `applier` - Callback to apply changes
///
/// # Returns
/// * `Result<(R, Vec<TreeChange>, Option<Vec<std::path::PathBuf>>), E>` - Result of the callback,
///   the changes made, and the files that were changed
pub fn apply_or_revert<R, E>(
    local_tree: &WorkingTree,
    subpath: &std::path::Path,
    basis_tree: &dyn Tree,
    dirty_tracker: Option<&mut DirtyTreeTracker>,
    applier: impl FnOnce(&std::path::Path) -> Result<R, E>,
) -> Result<(R, Vec<TreeChange>, Option<Vec<std::path::PathBuf>>), ApplyError<R, E>> {
    let r = match applier(local_tree.abspath(subpath).unwrap().as_path()) {
        Ok(r) => r,
        Err(e) => {
            reset_tree_with_dirty_tracker(
                local_tree,
                Some(basis_tree),
                Some(subpath),
                dirty_tracker,
            )
            .unwrap();
            return Err(ApplyError::CallbackError(e));
        }
    };

    let specific_files = if let Some(relpaths) = dirty_tracker.and_then(|x| x.relpaths()) {
        let mut relpaths: Vec<_> = relpaths.into_iter().collect();
        relpaths.sort();
        // Sort paths so that directories get added before the files they
        // contain (on VCSes where it matters)
        local_tree.add(
            relpaths
                .iter()
                .filter_map(|p| {
                    if local_tree.has_filename(p) && local_tree.is_ignored(p).is_some() {
                        Some(p.as_path())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .as_slice(),
        )?;
        let specific_files = relpaths
            .into_iter()
            .filter(|p| local_tree.is_versioned(p))
            .collect::<Vec<_>>();
        if specific_files.is_empty() {
            return Err(ApplyError::NoChanges(r));
        }
        Some(specific_files)
    } else {
        local_tree.smart_add(&[local_tree.abspath(subpath).unwrap().as_path()])?;
        if subpath.as_os_str().is_empty() {
            None
        } else {
            Some(vec![subpath.to_path_buf()])
        }
    };

    if local_tree.supports_setting_file_ids() {
        let local_lock = local_tree.lock_read().unwrap();
        let basis_lock = basis_tree.lock_read().unwrap();
        breezyshim::rename_map::guess_renames(basis_tree, local_tree).unwrap();
        std::mem::drop(basis_lock);
        std::mem::drop(local_lock);
    }

    let specific_files_ref = specific_files
        .as_ref()
        .map(|fs| fs.iter().map(|p| p.as_path()).collect::<Vec<_>>());

    let changes = local_tree
        .iter_changes(
            basis_tree,
            specific_files_ref.as_deref(),
            Some(false),
            Some(true),
        )?
        .collect::<Result<Vec<_>, _>>()?;

    if local_tree.get_parent_ids()?.len() <= 1 && changes.is_empty() {
        return Err(ApplyError::NoChanges(r));
    }

    Ok((r, changes, specific_files))
}

pub enum ChangelogError {
    NotDebianPackage(std::path::PathBuf),
    #[cfg(feature = "python")]
    Python(pyo3::PyErr),
}

impl std::fmt::Display for ChangelogError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ChangelogError::NotDebianPackage(path) => {
                write!(f, "Not a Debian package: {}", path.display())
            }
            #[cfg(feature = "python")]
            ChangelogError::Python(e) => write!(f, "{}", e),
        }
    }
}

#[cfg(feature = "python")]
impl From<pyo3::PyErr> for ChangelogError {
    fn from(e: pyo3::PyErr) -> Self {
        use pyo3::import_exception;

        import_exception!(breezy.transport, NoSuchFile);

        pyo3::Python::with_gil(|py| {
            if e.is_instance_of::<NoSuchFile>(py) {
                return ChangelogError::NotDebianPackage(
                    e.into_value(py)
                        .bind(py)
                        .getattr("path")
                        .unwrap()
                        .extract()
                        .unwrap(),
                );
            } else {
                ChangelogError::Python(e)
            }
        })
    }
}

/// Add an entry to a changelog.
///
/// # Arguments
/// * `working_tree` - Working tree
/// * `changelog_path` - Path to the changelog
/// * `entry` - Changelog entry
pub fn add_changelog_entry(
    working_tree: &WorkingTree,
    changelog_path: &std::path::Path,
    entry: &[&str],
) -> Result<(), crate::editor::EditorError> {
    use crate::editor::{Editor, MutableTreeEdit};
    let mut cl =
        working_tree.edit_file::<debian_changelog::ChangeLog>(changelog_path, false, true)?;

    cl.auto_add_change(
        entry,
        debian_changelog::get_maintainer().unwrap(),
        None,
        None,
    );

    cl.commit()?;

    Ok(())
}

#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    Default,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum Certainty {
    #[serde(rename = "possible")]
    Possible,
    #[serde(rename = "likely")]
    Likely,
    #[serde(rename = "confident")]
    Confident,
    #[default]
    #[serde(rename = "certain")]
    Certain,
}

impl std::str::FromStr for Certainty {
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

impl std::fmt::Display for Certainty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Certainty::Certain => write!(f, "certain"),
            Certainty::Confident => write!(f, "confident"),
            Certainty::Likely => write!(f, "likely"),
            Certainty::Possible => write!(f, "possible"),
        }
    }
}

#[cfg(feature = "python")]
impl pyo3::FromPyObject<'_> for Certainty {
    fn extract_bound(ob: &pyo3::Bound<pyo3::PyAny>) -> pyo3::PyResult<Self> {
        use std::str::FromStr;
        let s = ob.extract::<String>()?;
        Certainty::from_str(&s).map_err(pyo3::exceptions::PyValueError::new_err)
    }
}

#[cfg(feature = "python")]
impl pyo3::ToPyObject for Certainty {
    fn to_object(&self, py: pyo3::Python) -> pyo3::PyObject {
        self.to_string().to_object(py)
    }
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

/// Return the minimum certainty from a list of certainties.
pub fn min_certainty(certainties: &[Certainty]) -> Option<Certainty> {
    certainties.iter().min().cloned()
}

#[cfg(feature = "python")]
fn get_git_committer(working_tree: &WorkingTree) -> Option<String> {
    pyo3::prepare_freethreaded_python();
    pyo3::Python::with_gil(|py| {
        let repo = working_tree.branch().repository();
        let git = match repo.to_object(py).getattr(py, "_git") {
            Ok(x) => Some(x),
            Err(e) if e.is_instance_of::<pyo3::exceptions::PyAttributeError>(py) => None,
            Err(e) => {
                return Err(e);
            }
        };

        if let Some(git) = git {
            let cs = git.call_method0(py, "get_config_stack")?;

            let mut user = std::env::var("GIT_COMMITTER_NAME").ok();
            let mut email = std::env::var("GIT_COMMITTER_EMAIL").ok();
            if user.is_none() {
                match cs.call_method1(py, "get", (("user",), "name")) {
                    Ok(x) => {
                        user = Some(
                            std::str::from_utf8(x.extract::<&[u8]>(py)?)
                                .unwrap()
                                .to_string(),
                        );
                    }
                    Err(e) if e.is_instance_of::<pyo3::exceptions::PyKeyError>(py) => {
                        // Ignore
                    }
                    Err(e) => {
                        return Err(e);
                    }
                };
            }
            if email.is_none() {
                match cs.call_method1(py, "get", (("user",), "email")) {
                    Ok(x) => {
                        email = Some(
                            std::str::from_utf8(x.extract::<&[u8]>(py)?)
                                .unwrap()
                                .to_string(),
                        );
                    }
                    Err(e) if e.is_instance_of::<pyo3::exceptions::PyKeyError>(py) => {
                        // Ignore
                    }
                    Err(e) => {
                        return Err(e);
                    }
                };
            }

            if let (Some(user), Some(email)) = (user, email) {
                return Ok(Some(format!("{} <{}>", user, email)));
            }

            let gs = breezyshim::config::global_stack().unwrap();

            Ok(gs
                .get("email")?
                .map(|email| email.extract::<String>(py).unwrap()))
        } else {
            Ok(None)
        }
    })
    .unwrap()
}

/// Get the committer string for a tree
pub fn get_committer(working_tree: &WorkingTree) -> String {
    #[cfg(feature = "python")]
    if let Some(committer) = get_git_committer(working_tree) {
        return committer;
    }

    let config = working_tree.branch().get_config_stack();

    config
        .get("email")
        .unwrap()
        .map(|x| x.to_string())
        .unwrap_or_default()
}

/// Check whether there are any control files present in a tree.
///
/// # Arguments
///
///   * `tree`: tree to check
///   * `subpath`: subpath to check
///
/// # Returns
///
/// whether control file is present
pub fn control_file_present(tree: &dyn Tree, subpath: &std::path::Path) -> bool {
    for name in [
        "debian/control",
        "debian/control.in",
        "control",
        "control.in",
        "debian/debcargo.toml",
    ] {
        let name = subpath.join(name);
        if tree.has_filename(name.as_path()) {
            return true;
        }
    }
    false
}

pub fn is_debcargo_package(tree: &dyn Tree, subpath: &std::path::Path) -> bool {
    tree.has_filename(subpath.join("debian/debcargo.toml").as_path())
}

pub fn control_files_in_root(tree: &dyn Tree, subpath: &std::path::Path) -> bool {
    let debian_path = subpath.join("debian");
    if tree.has_filename(debian_path.as_path()) {
        return false;
    }

    let control_path = subpath.join("control");
    if tree.has_filename(control_path.as_path()) {
        return true;
    }

    tree.has_filename(subpath.join("control.in").as_path())
}

pub fn parseaddr(input: &str) -> Option<(Option<String>, Option<String>)> {
    if let Some((_whole, name, addr)) =
        lazy_regex::regex_captures!(r"(?:(?P<name>[^<]*)\s*<)?(?P<addr>[^<>]*)>?", input)
    {
        let name = match name.trim() {
            "" => None,
            x => Some(x.to_string()),
        };
        let addr = match addr.trim() {
            "" => None,
            x => Some(x.to_string()),
        };

        return Some((name, addr));
    } else if let Some((_whole, addr)) = lazy_regex::regex_captures!(r"(?P<addr>[^<>]*)", input) {
        let addr = Some(addr.trim().to_string());

        return Some((None, addr));
    } else if input.is_empty() {
        return None;
    } else if !input.contains('<') {
        return Some((None, Some(input.to_string())));
    }
    None
}

pub fn gbp_dch(path: &std::path::Path) -> Result<(), std::io::Error> {
    let mut cmd = std::process::Command::new("gbp");
    cmd.arg("dch").arg("--ignore-branch");
    cmd.current_dir(path);
    let status = cmd.status()?;
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("gbp dch failed: {}", status),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_parseaddr() {
        assert_eq!(
            parseaddr("foo <bar@example.com>").unwrap(),
            (Some("foo".to_string()), Some("bar@example.com".to_string()))
        );
        assert_eq!(parseaddr("foo").unwrap(), (None, Some("foo".to_string())));
    }

    #[cfg(feature = "python")]
    #[serial]
    #[test]
    fn test_git_env() {
        let td = tempfile::tempdir().unwrap();
        let cd = breezyshim::controldir::create_standalone_workingtree(td.path(), "git").unwrap();

        let old_name = std::env::var("GIT_COMMITTER_NAME").ok();
        let old_email = std::env::var("GIT_COMMITTER_EMAIL").ok();

        std::env::set_var("GIT_COMMITTER_NAME", "Some Git Committer");
        std::env::set_var("GIT_COMMITTER_EMAIL", "committer@example.com");

        let committer = get_committer(&cd);

        if let Some(old_name) = old_name {
            std::env::set_var("GIT_COMMITTER_NAME", old_name);
        } else {
            std::env::remove_var("GIT_COMMITTER_NAME");
        }

        if let Some(old_email) = old_email {
            std::env::set_var("GIT_COMMITTER_EMAIL", old_email);
        } else {
            std::env::remove_var("GIT_COMMITTER_EMAIL");
        }

        assert_eq!("Some Git Committer <committer@example.com>", committer);
    }

    #[serial]
    #[test]
    fn test_git_config() {
        let td = tempfile::tempdir().unwrap();
        let cd = breezyshim::controldir::create_standalone_workingtree(td.path(), "git").unwrap();

        std::fs::write(
            td.path().join(".git/config"),
            b"[user]\nname = Some Git Committer\nemail = other@example.com",
        )
        .unwrap();

        assert_eq!(get_committer(&cd), "Some Git Committer <other@example.com>");
    }

    #[test]
    fn test_min_certainty() {
        assert_eq!(None, min_certainty(&[]));
        assert_eq!(
            Some(Certainty::Certain),
            min_certainty(&[Certainty::Certain])
        );
        assert_eq!(
            Some(Certainty::Possible),
            min_certainty(&[Certainty::Possible])
        );
        assert_eq!(
            Some(Certainty::Possible),
            min_certainty(&[Certainty::Possible, Certainty::Certain])
        );
        assert_eq!(
            Some(Certainty::Likely),
            min_certainty(&[Certainty::Likely, Certainty::Certain])
        );
        assert_eq!(
            Some(Certainty::Possible),
            min_certainty(&[Certainty::Likely, Certainty::Certain, Certainty::Possible])
        );
    }
}
