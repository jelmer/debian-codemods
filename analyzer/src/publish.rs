//! Publishing utilities for updating Vcs-* headers and creating VCS repositories.
use crate::salsa::guess_repository_url;
use crate::vcs::determine_browser_url;
use crate::{get_committer, parseaddr};
use debian_control::control::Source;

use breezyshim::error::Error as BrzError;
use breezyshim::forge::create_project;
use breezyshim::tree::WorkingTree;
use breezyshim::workspace::check_clean_tree;
use debian_control::vcs::ParsedVcs;
use std::path::Path;
use url::Url;

/// Update the Vcs-* headers in the control file for the given source package.
pub fn update_control_for_vcs_url(
    source: &mut Source,
    vcs_type: breezyshim::foreign::VcsType,
    vcs_url: &str,
) {
    source.as_mut_deb822().insert(
        match vcs_type {
            breezyshim::foreign::VcsType::Git => "Vcs-Git",
            breezyshim::foreign::VcsType::Bazaar => "Vcs-Bzr",
        },
        vcs_url,
    );
    if let Some(url) = determine_browser_url("git", vcs_url, None) {
        source.as_mut_deb822().insert("Vcs-Browser", url.as_ref());
    } else {
        source.as_mut_deb822().remove("Vcs-Browser");
    }
}

/// Create a VCS repository for the given source package.
pub fn create_vcs_url(repo_url: &Url, summary: Option<&str>) -> Result<(), BrzError> {
    match create_project(repo_url.as_str(), summary) {
        Ok(()) => {
            log::info!("Created {}", repo_url);
            Ok(())
        }
        Err(BrzError::ForgeProjectExists(..)) | Err(BrzError::AlreadyControlDir(..)) => {
            log::debug!("{} already exists", repo_url);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Error type for the publish module.
#[derive(Debug, Clone)]
pub enum Error {
    /// No Vcs-* location specified.
    NoVcsLocation,
    /// File not found.
    FileNotFound(std::path::PathBuf),
    /// Conflicting Vcs-* location already specified.
    ConflictingVcsAlreadySpecified(String, String, String),
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

/// Update the official VCS location for the given source package.
pub fn update_official_vcs(
    wt: &WorkingTree,
    subpath: &Path,
    repo_url: Option<&Url>,
    branch: Option<&str>,
    committer: Option<&str>,
    force: Option<bool>,
) -> Result<ParsedVcs, Error> {
    let force = force.unwrap_or(false);
    // TODO(jelmer): Allow creation of the repository as well
    check_clean_tree(wt, &wt.basis_tree().unwrap(), subpath).unwrap();

    let debian_path = subpath.join("debian");
    let subpath = match subpath.to_string_lossy().as_ref() {
        "" | "." => None,
        _ => Some(subpath.to_path_buf()),
    };
    let control_path = debian_path.join("control");

    let editor = match crate::control::TemplatedControlEditor::open(&control_path) {
        Ok(e) => e,
        Err(crate::editor::EditorError::IoError(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::FileNotFound(control_path));
        }
        Err(e) => panic!("Failed to open control file: {:?}", e),
    };
    let mut source = editor.source().unwrap();

    if let Some(package_vcs) = crate::vcs::source_package_vcs(&source) {
        let vcs_type = package_vcs.type_str();
        let existing: ParsedVcs = package_vcs.clone().into();
        let actual = ParsedVcs {
            repo_url: repo_url.unwrap().to_string(),
            branch: branch.map(|s| s.to_string()),
            subpath: subpath.map(|p| p.to_string_lossy().to_string()),
        };
        if existing != actual && !force {
            return Err(Error::ConflictingVcsAlreadySpecified(
                vcs_type.to_owned(),
                existing.to_string(),
                actual.to_string(),
            ));
        }
        log::debug!("Using existing URL {}", existing);
        return Ok(existing);
    }
    let maintainer_email = parseaddr(source.maintainer().unwrap().as_str())
        .unwrap()
        .1
        .unwrap();
    let source_name = source.name().unwrap();
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
    let branch = wt.branch();

    let branch_name = match branch.vcs_type() {
        breezyshim::foreign::VcsType::Git => Some("debian/main"),
        breezyshim::foreign::VcsType::Bazaar => None,
    };

    let vcs_url = ParsedVcs {
        repo_url: repo_url.to_string(),
        branch: branch_name.map(|s| s.to_string()),
        subpath: subpath.map(|p| p.to_string_lossy().to_string()),
    };
    update_control_for_vcs_url(&mut source, branch.vcs_type(), &vcs_url.to_string());
    let parsed_vcs = vcs_url.clone();

    let committer = committer.map_or_else(|| get_committer(wt), |s| s.to_string());

    match wt
        .build_commit()
        .message("Set Vcs headers.")
        .allow_pointless(false)
        .committer(committer.as_str())
        .commit()
    {
        Ok(_) | Err(BrzError::PointlessCommit) => {}
        Err(e) => {
            panic!("Failed to commit: {:?}", e);
        }
    }

    Ok(parsed_vcs)
}
