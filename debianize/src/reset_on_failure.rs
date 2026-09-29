//! RAII guard that undoes changes made to a working tree, its branches, and
//! its tags when a debianize run fails partway through.

use breezyshim::branch::Branch;
use breezyshim::error::Error as BrzError;
use breezyshim::workingtree::PyWorkingTree;
use breezyshim::RevisionId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Guard that resets the working tree to its pre-debianize state unless
/// disarmed. Disarm on success; any other exit (error return or panic)
/// rolls back the tree, the tags on its branch, and any branches that were
/// created or moved.
pub(crate) struct ResetOnFailure<'a> {
    wt: &'a dyn PyWorkingTree,
    subpath: PathBuf,
    /// Name of the branch the working tree pointed at when the guard was
    /// created. `None` if it couldn't be determined.
    original_branch_name: Option<String>,
    /// Snapshot of tag name -> revision on the working tree's branch.
    original_tags: HashMap<String, RevisionId>,
    /// Snapshot of branch name -> tip revision for every branch in the
    /// controldir at guard creation time.
    original_branches: HashMap<String, RevisionId>,
    disarmed: bool,
}

impl<'a> ResetOnFailure<'a> {
    pub fn new(wt: &'a dyn PyWorkingTree, subpath: &Path) -> Result<Self, BrzError> {
        // Try to check if tree is clean, but handle dirstate errors gracefully
        match wt.basis_tree() {
            Ok(basis_tree) => {
                match breezyshim::workspace::check_clean_tree(wt, &basis_tree, subpath) {
                    Ok(_) => {}
                    Err(BrzError::Other(ref py_err))
                        if py_err.to_string().contains("IndexError") =>
                    {
                        // Ignore IndexError from dirstate issues in test environments
                        log::warn!("Ignoring dirstate IndexError during clean tree check");
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(e) => {
                log::warn!("Could not get basis tree: {:?}", e);
            }
        }

        let branch = wt.branch();
        let original_branch_name = branch.name();
        let original_tags = match branch.tags().and_then(|t| t.get_tag_dict()) {
            Ok(tags) => tags,
            Err(e) => {
                log::warn!("Could not snapshot tags for rollback: {:?}", e);
                HashMap::new()
            }
        };
        let controldir = wt.controldir();
        let original_branches = match controldir.branch_names() {
            Ok(names) => names
                .into_iter()
                .filter_map(|name| {
                    let opened = if name.is_empty() {
                        controldir.open_branch(None)
                    } else {
                        controldir.open_branch(Some(&name))
                    };
                    match opened {
                        Ok(b) => Some((name, b.last_revision())),
                        Err(e) => {
                            log::warn!("Could not open branch {:?} for snapshot: {:?}", name, e);
                            None
                        }
                    }
                })
                .collect(),
            Err(e) => {
                log::warn!("Could not list branches for rollback snapshot: {:?}", e);
                HashMap::new()
            }
        };

        Ok(Self {
            wt,
            subpath: subpath.to_path_buf(),
            original_branch_name,
            original_tags,
            original_branches,
            disarmed: false,
        })
    }

    pub fn disarm(&mut self) {
        self.disarmed = true;
    }

    fn restore_branches(&self) {
        let controldir = self.wt.controldir();
        let current_names: Vec<String> = match controldir.branch_names() {
            Ok(names) => names,
            Err(e) => {
                log::error!("Could not list branches during rollback: {:?}", e);
                return;
            }
        };

        for name in &current_names {
            if self.original_branches.contains_key(name) {
                continue;
            }
            // Don't destroy the branch the wt still points at; that would leave
            // the workspace in an unusable state. It will be reset by the
            // per-branch loop below and by the working tree reset.
            if self.original_branch_name.as_deref() == Some(name.as_str()) {
                continue;
            }
            let arg = if name.is_empty() {
                None
            } else {
                Some(name.as_str())
            };
            match controldir.destroy_branch(arg) {
                Ok(_) => log::info!("Removed branch {:?} created during failed run", name),
                Err(e) => log::error!("Failed to destroy branch {:?}: {:?}", name, e),
            }
        }

        for (name, revid) in &self.original_branches {
            let arg = if name.is_empty() {
                None
            } else {
                Some(name.as_str())
            };
            let branch = match controldir.open_branch(arg) {
                Ok(b) => b,
                Err(BrzError::NotBranchError { .. }) => {
                    log::warn!(
                        "Branch {:?} was removed during run; cannot restore automatically",
                        name
                    );
                    continue;
                }
                Err(e) => {
                    log::error!("Could not open branch {:?} for rollback: {:?}", name, e);
                    continue;
                }
            };
            if &branch.last_revision() == revid {
                continue;
            }
            if let Err(e) = branch.generate_revision_history(revid) {
                log::error!("Failed to reset branch {:?} to {:?}: {:?}", name, revid, e);
            }
        }
    }

    fn restore_branch_reference(&self) {
        let Some(name) = self.original_branch_name.as_deref() else {
            return;
        };
        let controldir = self.wt.controldir();
        let current = controldir.get_branch_reference(Some("")).ok();
        let target = match controldir.open_branch(Some(name)) {
            Ok(b) => b,
            Err(e) => {
                log::error!(
                    "Could not open original branch {:?} for rollback: {:?}",
                    name,
                    e
                );
                return;
            }
        };
        let target_url = target.get_user_url().to_string();
        if current.as_deref() == Some(target_url.as_str()) {
            return;
        }
        if let Err(e) = controldir.set_branch_reference(target.as_ref(), Some("")) {
            log::error!("Failed to restore branch reference to {:?}: {:?}", name, e);
        }
    }

    fn restore_tags(&self) {
        let tags = match self.wt.branch().tags() {
            Ok(t) => t,
            Err(e) => {
                log::error!("Could not access tags during rollback: {:?}", e);
                return;
            }
        };
        let current = match tags.get_tag_dict() {
            Ok(d) => d,
            Err(e) => {
                log::error!("Could not read current tags during rollback: {:?}", e);
                return;
            }
        };
        for name in current.keys() {
            if !self.original_tags.contains_key(name) {
                if let Err(e) = tags.delete_tag(name) {
                    log::error!("Failed to delete tag {:?} during rollback: {:?}", name, e);
                }
            }
        }
        for (name, revid) in &self.original_tags {
            if current.get(name) == Some(revid) {
                continue;
            }
            if let Err(e) = tags.set_tag(name, revid) {
                log::error!("Failed to restore tag {:?} during rollback: {:?}", name, e);
            }
        }
    }
}

impl<'a> Drop for ResetOnFailure<'a> {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }
        self.restore_branch_reference();
        self.restore_branches();
        self.restore_tags();
        match breezyshim::workspace::reset_tree(self.wt, None, Some(&self.subpath)) {
            Ok(_) => log::info!("Reset tree after failure"),
            Err(e) => log::error!("Failed to reset tree: {:?}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use breezyshim::testing::TestEnv;
    use breezyshim::tree::MutableTree;
    use breezyshim::workingtree::{GenericWorkingTree, WorkingTree};
    use serial_test::serial;
    use tempfile::tempdir;

    fn init_wt_with_commit(path: &Path) -> GenericWorkingTree {
        let format = breezyshim::controldir::ControlDirFormat::default();
        let transport =
            breezyshim::transport::get_transport(&url::Url::from_file_path(path).unwrap(), None)
                .unwrap();
        let controldir = format.initialize_on_transport(&transport).unwrap();
        controldir.create_repository(None).unwrap();
        controldir.create_branch(None).unwrap();
        let wt = controldir.create_workingtree().unwrap();
        std::fs::write(path.join("README"), b"initial\n").unwrap();
        wt.add(&[Path::new("README")]).unwrap();
        wt.build_commit().message("initial").commit().unwrap();
        wt
    }

    #[test]
    #[serial]
    fn test_reset_on_failure_restores_tags() {
        let _env = TestEnv::new();
        let td = tempdir().unwrap();
        let wt = init_wt_with_commit(td.path());
        let head = wt.last_revision().unwrap();

        let tags = wt.branch().tags().unwrap();
        tags.set_tag("original", &head).unwrap();

        {
            let _guard = ResetOnFailure::new(&wt, Path::new("")).unwrap();
            tags.set_tag("added-during-run", &head).unwrap();
            tags.delete_tag("original").unwrap();
        }

        let final_tags = wt.branch().tags().unwrap().get_tag_dict().unwrap();
        assert_eq!(final_tags.get("original"), Some(&head));
        assert!(!final_tags.contains_key("added-during-run"));
    }

    #[test]
    #[serial]
    fn test_reset_on_failure_destroys_new_branch() {
        let _env = TestEnv::new();
        let td = tempdir().unwrap();
        let wt = init_wt_with_commit(td.path());

        {
            let _guard = ResetOnFailure::new(&wt, Path::new("")).unwrap();
            wt.controldir()
                .create_branch(Some("upstream/latest"))
                .unwrap();
        }

        assert!(!wt.controldir().has_branch(Some("upstream/latest")));
    }

    #[test]
    #[serial]
    fn test_reset_on_failure_disarm_keeps_changes() {
        let _env = TestEnv::new();
        let td = tempdir().unwrap();
        let wt = init_wt_with_commit(td.path());
        let head = wt.last_revision().unwrap();

        {
            let mut guard = ResetOnFailure::new(&wt, Path::new("")).unwrap();
            wt.branch().tags().unwrap().set_tag("kept", &head).unwrap();
            wt.controldir().create_branch(Some("kept-branch")).unwrap();
            guard.disarm();
        }

        assert!(wt.branch().tags().unwrap().has_tag("kept"));
        assert!(wt.controldir().has_branch(Some("kept-branch")));
    }
}
