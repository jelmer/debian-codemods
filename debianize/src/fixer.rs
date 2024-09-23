use crate::simple_apt_repo::SimpleTrustedAptRepo;
use crate::DebianizePreferences;
use breezyshim::error::Error as BrzError;
use breezyshim::workingtree::WorkingTree;
use buildlog_consultant::Problem;
use ognibuild::buildlog::problem_to_dependency;
use ognibuild::debian::build::BuildOnceResult;
use ognibuild::debian::context::{Error, Phase};
use ognibuild::debian::fix_build::{DebianBuildFixer, IterateBuildError};
use ognibuild::dependencies::debian::DebianDependency;
use ognibuild::dependency::Dependency;
use ognibuild::fix_build::InterimError;
use ognibuild::upstream::find_upstream;
use std::path::Path;

/// Fixer that invokes debianize to create a package.
pub struct DebianizeFixer<'a> {
    vcs_directory: std::path::PathBuf,
    apt_repo: SimpleTrustedAptRepo,
    do_build: Box<
        dyn for<'b, 'c, 'd> Fn(
            &'b WorkingTree,
            &'c std::path::Path,
            &'d std::path::Path,
            Vec<&str>,
        ) -> Result<BuildOnceResult, IterateBuildError>,
    >,
    preferences: &'a DebianizePreferences,
}

impl<'a> DebianizeFixer<'a> {
    pub fn new(
        vcs_directory: std::path::PathBuf,
        apt_repo: SimpleTrustedAptRepo,
        do_build: Box<
            dyn for<'b, 'c, 'd> Fn(
                &'b WorkingTree,
                &'c std::path::Path,
                &'d std::path::Path,
                Vec<&str>,
            ) -> Result<BuildOnceResult, IterateBuildError>,
        >,
        preferences: &'a DebianizePreferences,
    ) -> Self {
        Self {
            vcs_directory,
            apt_repo,
            do_build,
            preferences,
        }
    }

    pub fn apt_repo(&self) -> &SimpleTrustedAptRepo {
        &self.apt_repo
    }
}

impl<'a> std::fmt::Debug for DebianizeFixer<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "DebianizeFixer")
    }
}

impl<'a> std::fmt::Display for DebianizeFixer<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "DebianizeFixer")
    }
}

impl<'a> DebianBuildFixer for DebianizeFixer<'a> {
    fn can_fix(&self, problem: &dyn Problem) -> bool {
        let dep = if let Some(dep) = problem_to_dependency(problem) {
            dep
        } else {
            return false;
        };

        find_upstream(dep.as_ref()).is_some()
    }

    fn fix(&self, problem: &dyn Problem, phase: &Phase) -> Result<bool, InterimError<Error>> {
        let dep = match problem_to_dependency(problem) {
            Some(dep) => dep,
            None => {
                log::error!("Unable to convert problem {:?} to dependency", problem);
                return Ok(false);
            }
        };
        log::debug!("Translated problem {:?} to requirement {:?}", problem, dep);
        let upstream_info = if let Some(upstream_info) = find_upstream(dep.as_ref()) {
            upstream_info
        } else {
            log::error!(
                "Unable to find upstream information for requirement {:?}",
                dep,
            );
            return Ok(false);
        };
        let (upstream_branch, upstream_subpath) = if let Some(url) = upstream_info.repository() {
            log::info!("Packaging {:?} to address {:?}", url, problem);

            // TODO: use the branch name from the upstream info, if present
            let url: url::Url = url.parse().unwrap();

            let upstream_branch = match breezyshim::branch::open(&url) {
                Ok(branch) => Some(branch),
                Err(e @ BrzError::NotBranchError { .. }) => {
                    log::warn!("Unable to open branch {}: {}", url, e);
                    None
                }
                Err(e) => panic!("Unexpected error opening branch: {:?}", e),
            };
            (upstream_branch, None)
        } else {
            (None, None)
        };
        let vcs_path = if let Some(name) = upstream_info.name() {
            self.vcs_directory.join(name.replace("/", "-"))
        } else {
            panic!("no upstream name provided");
        };
        if vcs_path.exists() {
            std::fs::remove_dir_all(&vcs_path).map_err(|e| InterimError::Other(e.into()))?;
        }
        let format = if let Some(upstream_branch) = upstream_branch.as_ref() {
            Some(upstream_branch.controldir().cloning_metadir())
        } else {
            // TODO(jelmer): default to git?
            None
        };
        let result = breezyshim::controldir::create_branch_convenience(
            &url::Url::from_directory_path(vcs_path).unwrap(),
            Some(true),
            &format.unwrap_or_default(),
        )
        .unwrap();
        let new_wt = result.controldir().open_workingtree().unwrap();
        let new_subpath = Path::new("");
        match crate::debianize(
            &new_wt,
            new_subpath,
            upstream_branch.as_deref(),
            upstream_subpath,
            self.preferences,
            upstream_info.version(),
            &upstream_info,
        ) {
            Ok(_) => {}
            Err(e) => {
                log::error!("Failed to debianize: {:?}", e);
                return Ok(false);
            }
        }
        match (self.do_build)(
            &new_wt,
            new_subpath,
            self.apt_repo.directory(),
            self.apt_repo
                .sources_lines()
                .iter()
                .map(|s| s.as_str())
                .collect(),
        ) {
            Ok(_) => {}
            Err(e) => {
                log::error!("Failed to build: {:?}", e);
                return Ok(false);
            }
        }
        self.apt_repo
            .refresh()
            .map_err(|e| InterimError::Other(e.into()))?;
        Ok(true)
    }
}
