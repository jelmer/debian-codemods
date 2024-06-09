use crate::debmutateshim::{
    edit_control, source_package_vcs, split_vcs_url, unsplit_vcs_url, ControlEditor,
    ControlLikeEditor, Deb822Paragraph, DebcargoControlShimEditor,
};
use crate::salsa::guess_repository_url;
use crate::vcs::determine_browser_url;
use crate::{branch_vcs_type, get_committer, parseaddr};

use breezyshim::forge::{create_project, Error as ForgeError};
use breezyshim::tree::{CommitError, Tree, WorkingTree};
use breezyshim::workspace::check_clean_tree;
use std::path::Path;
use url::Url;

pub fn update_control_for_vcs_url(
    source: &mut Deb822Paragraph,
    vcs_type: &str,
    vcs_url: &url::Url,
) {
    source.set(format!("Vcs-{}", vcs_type).as_str(), vcs_url.as_str());
    if let Some(url) = determine_browser_url("git", vcs_url, None) {
        source.set("Vcs-Browser", url.as_str());
    } else {
        source.remove("Vcs-Browser");
    }
}

pub fn create_vcs_url(repo_url: &Url, summary: Option<&str>) -> Result<(), ForgeError> {
    match create_project(repo_url.as_str(), summary) {
        Ok(()) => {
            log::info!("Created {}", repo_url);
            Ok(())
        }
        Err(ForgeError::ProjectExists(_n)) => {
            log::debug!("{} already exists", repo_url);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[derive(Debug, Clone)]
pub enum Error {
    NoVcsLocation,
    FileNotFound(std::path::PathBuf),
    ConflictingVcsAlreadySpecified(String, Url, Url),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use Error::*;
        match self {
            NoVcsLocation => write!(f, "No Vcs-* location specified"),
            FileNotFound(path) => write!(f, "File not found: {}", path.display()),
            ConflictingVcsAlreadySpecified(_vcs_type, existing_url, new_url) => write!(
                f,
                "Conflicting Vcs-* location already specified: {} vs {}",
                existing_url, new_url
            ),
        }
    }
}

pub fn update_official_vcs(
    wt: &WorkingTree,
    subpath: &Path,
    repo_url: Option<&Url>,
    branch: Option<&str>,
    committer: Option<&str>,
    force: Option<bool>,
) -> Result<(Url, Option<String>, Option<std::path::PathBuf>), Error> {
    let force = force.unwrap_or(false);
    // TODO(jelmer): Allow creation of the repository as well
    check_clean_tree(wt, wt.basis_tree().as_ref(), subpath).unwrap();

    let debian_path = subpath.join("debian");
    let subpath = match subpath.to_string_lossy().as_ref() {
        "" | "." => None,
        _ => Some(subpath.to_path_buf()),
    };
    let debcargo_path = debian_path.join("debcargo.toml");
    let control_path = debian_path.join("control");

    let mut editor: Box<dyn ControlLikeEditor> = if wt.has_filename(debcargo_path.as_path()) {
        Box::new(DebcargoControlShimEditor::from_debian_dir(
            wt.abspath(debian_path.as_path()).unwrap().as_path(),
        ))
    } else if wt.has_filename(control_path.as_path()) {
        let control_path = wt.abspath(control_path.as_path()).unwrap();
        Box::new(ControlEditor::open(Some(control_path.as_path()), None))
            as Box<dyn ControlLikeEditor>
    } else {
        return Err(Error::FileNotFound(control_path));
    };
    let (repo_url, branch, subpath) = edit_control(editor.as_mut(), |e| {
        let mut source = e.source().unwrap();

        if let Some((vcs_type, existing_url)) = source_package_vcs(&source) {
            let (existing_repo_url, existing_branch, existing_subpath) =
                split_vcs_url(&existing_url);
            let existing = (existing_repo_url, existing_branch, existing_subpath);
            if let Some(repo_url) = repo_url {
                if existing
                    != (
                        repo_url.clone(),
                        branch.map(|s| s.to_string()),
                        subpath.clone(),
                    )
                    && !force
                {
                    return Err(Error::ConflictingVcsAlreadySpecified(
                        vcs_type,
                        existing_url,
                        unsplit_vcs_url(repo_url, branch, subpath.as_deref()),
                    ));
                }
            }
            log::debug!("Using existing URL {}", existing_url);
            return Ok(existing);
        }
        let maintainer_email = parseaddr(source.get("Maintainer").unwrap().as_str())
            .unwrap()
            .1
            .unwrap();
        let source_name = source.get("Source").unwrap();
        let mut repo_url = repo_url.map(|u| u.to_owned());
        if repo_url.is_none() {
            repo_url = guess_repository_url(source_name.as_str(), maintainer_email.as_str());
        }
        let repo_url = match repo_url {
            Some(url) => url,
            None => {
                return Err(Error::NoVcsLocation);
            }
        };
        log::info!("Using repository URL: {}", repo_url);
        // TODO(jelmer): Detect vcs type in a better way
        let branch = wt.branch();
        let vcs_type = branch_vcs_type(branch.as_ref());

        let branch = match vcs_type.as_str() {
            "git" => Some("debian/main"),
            "bzr" => None,
            _ => {
                panic!("Unknown VCS type");
            }
        };

        let vcs_url = unsplit_vcs_url(&repo_url, branch, subpath.as_deref());
        update_control_for_vcs_url(&mut source, vcs_type.as_str(), &vcs_url);
        Ok((repo_url, branch.map(|s| s.to_string()), subpath))
    })?;

    let committer = committer.map_or_else(|| get_committer(wt), |s| s.to_string());

    match wt.commit(
        "Set Vcs headers.",
        Some(false),
        Some(committer.as_str()),
        None,
    ) {
        Ok(_) | Err(CommitError::PointlessCommit) => {}
        Err(e) => {
            panic!("Failed to commit: {:?}", e);
        }
    }

    Ok((repo_url, branch, subpath))
}
