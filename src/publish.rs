use crate::debmutateshim::Deb822Paragraph;
use crate::vcs::determine_browser_url;
use breezyshim::forge::{create_project, Error};
use breezyshim::tree::WorkingTree;
use std::path::Path;
use url::Url;

pub fn update_control_for_vcs_url(
    source: &mut Deb822Paragraph,
    vcs_type: &str,
    vcs_url: &url::Url,
) {
    source.set(format!("Vcs-{}", vcs_type).as_str(), vcs_url.as_str());
    if let Some(url) = determine_browser_url("git", &vcs_url) {
        source.set("Vcs-Browser", url.as_str());
    } else {
        source.remove("Vcs-Browser");
    }
}

pub fn create_vcs_url(repo_url: &Url, branch: Option<&str>) -> Result<(), Error> {
    match create_project(repo_url) {
        Ok(()) => {
            log::info!("Created {}", repo_url);
            Ok(())
        }
        Err(Error::ProjectExists(n)) => {
            log::debug!("{} already exists", repo_url);
            Ok(())
        }
        Err(e) => Err(e),
    }
}
