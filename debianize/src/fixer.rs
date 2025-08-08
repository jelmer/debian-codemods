use crate::simple_apt_repo::SimpleTrustedAptRepo;
use crate::DebianizePreferences;
use breezyshim::branch::{Branch, GenericBranch, PyBranch};
use breezyshim::error::Error as BrzError;
use breezyshim::workingtree::GenericWorkingTree;

/// Default VCS format for new repositories
const DEFAULT_VCS_FORMAT: &str = "git";

/// Extract branch name from URL if present
/// Supports common patterns like #branch=name, ?branch=name, or GitHub/GitLab tree URLs
pub fn extract_branch_from_url(url: &url::Url) -> Option<String> {
    // Check URL fragment (e.g., #branch=main)
    if let Some(fragment) = url.fragment() {
        if let Some(branch) = fragment.strip_prefix("branch=") {
            return Some(branch.to_string());
        }
    }

    // Check query parameters (e.g., ?branch=main)
    for (key, value) in url.query_pairs() {
        if key == "branch" {
            return Some(value.to_string());
        }
    }

    // Check for GitHub/GitLab tree URLs (e.g., /tree/branch_name)
    let path = url.path();
    if let Some(pos) = path.find("/tree/") {
        let branch_part = &path[pos + 6..];
        if let Some(end) = branch_part.find('/') {
            return Some(branch_part[..end].to_string());
        } else if !branch_part.is_empty() {
            return Some(branch_part.to_string());
        }
    }

    None
}
use buildlog_consultant::Problem;
use ognibuild::buildlog::problem_to_dependency;
use ognibuild::debian::build::BuildOnceResult;
use ognibuild::debian::context::{Error, Phase};
use ognibuild::debian::fix_build::{DebianBuildFixer, IterateBuildError};
use ognibuild::fix_build::InterimError;
use ognibuild::upstream::find_upstream;
use std::path::Path;

/// Fixer that invokes debianize to create a package.
pub struct DebianizeFixer<'a> {
    vcs_directory: std::path::PathBuf,
    apt_repo: SimpleTrustedAptRepo,
    do_build: Box<
        dyn for<'b, 'c, 'd> Fn(
            &'b GenericWorkingTree,
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
                &'b GenericWorkingTree,
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

    fn fix(&self, problem: &dyn Problem, _phase: &Phase) -> Result<bool, InterimError<Error>> {
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

            // Parse URL and extract branch information if present
            let mut url: url::Url = url.parse().unwrap();

            // Extract branch name if present in URL
            let branch_name = extract_branch_from_url(&url);

            // Clean up URL for branch opening (remove fragment/query that specified branch)
            if branch_name.is_some() {
                url.set_fragment(None);
                // Remove branch from query if it was there
                let filtered_query: String = url
                    .query_pairs()
                    .filter(|(k, _)| k != "branch")
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("&");
                url.set_query(if filtered_query.is_empty() {
                    None
                } else {
                    Some(&filtered_query)
                });
            }

            let upstream_branch = match breezyshim::branch::open(&url) {
                Ok(branch) => {
                    // If a specific branch was requested, try to switch to it
                    if let Some(branch_name) = &branch_name {
                        log::info!("Opening specific branch: {}", branch_name);
                        match branch.controldir().get_branches() {
                            Ok(mut branches) => {
                                if let Some(specific_branch) = branches.remove(branch_name) {
                                    log::info!("Successfully switched to branch: {}", branch_name);
                                    Some(specific_branch as Box<dyn Branch>)
                                } else {
                                    log::warn!(
                                        "Branch '{}' not found, using default branch",
                                        branch_name
                                    );
                                    Some(branch)
                                }
                            }
                            Err(e) => {
                                log::warn!(
                                    "Unable to get branches for {}: {}, using default",
                                    url,
                                    e
                                );
                                Some(branch)
                            }
                        }
                    } else {
                        Some(branch)
                    }
                }
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
            upstream_branch.controldir().cloning_metadir()
        } else {
            // Default to git format for new repositories
            use breezyshim::controldir::ControlDirFormatRegistry;
            let registry = ControlDirFormatRegistry::new();
            registry.make_controldir(DEFAULT_VCS_FORMAT).unwrap()
        };
        let result = breezyshim::controldir::create_branch_convenience(
            &url::Url::from_directory_path(vcs_path).unwrap(),
            Some(true),
            &format,
        )
        .unwrap();
        let new_wt = result.controldir().open_workingtree().unwrap();
        let new_subpath = Path::new("");
        match crate::debianize(
            &new_wt,
            new_subpath,
            upstream_branch.as_ref().and_then(|b| {
                // Try to downcast to GenericBranch which implements PyBranch
                b.as_any()
                    .downcast_ref::<GenericBranch>()
                    .map(|gb| gb as &dyn PyBranch)
            }),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_fixer_display_and_debug() {
        let td = tempfile::tempdir().unwrap();
        let apt_repo = crate::simple_apt_repo::SimpleTrustedAptRepo::new(td.path().to_path_buf());
        let prefs = DebianizePreferences::default();

        // Just test the fmt trait implementations
        let fixer = DebianizeFixer {
            vcs_directory: PathBuf::from("/tmp/vcs"),
            apt_repo,
            do_build: Box::new(|_, _, _, _| unreachable!()),
            preferences: &prefs,
        };

        assert_eq!(format!("{}", fixer), "DebianizeFixer");
        assert_eq!(format!("{:?}", fixer), "DebianizeFixer");
    }

    #[test]
    fn test_fixer_new() {
        let td = tempfile::tempdir().unwrap();
        let apt_repo = crate::simple_apt_repo::SimpleTrustedAptRepo::new(td.path().to_path_buf());
        let prefs = DebianizePreferences::default();

        // Test constructor
        let fixer = DebianizeFixer::new(
            PathBuf::from("/tmp/vcs"),
            apt_repo,
            Box::new(|_, _, _, _| {
                Ok(BuildOnceResult {
                    source_package: "test".to_string(),
                    version: "1.0-1".parse().unwrap(),
                    changes_names: vec![],
                })
            }),
            &prefs,
        );

        assert_eq!(fixer.vcs_directory, PathBuf::from("/tmp/vcs"));
        assert!(fixer.apt_repo().url().is_none()); // The repo isn't started yet
    }

    #[test]
    fn test_apt_repo_accessor() {
        let td = tempfile::tempdir().unwrap();
        let apt_repo = crate::simple_apt_repo::SimpleTrustedAptRepo::new(td.path().to_path_buf());
        let prefs = DebianizePreferences::default();

        let fixer = DebianizeFixer {
            vcs_directory: PathBuf::from("/tmp/vcs"),
            apt_repo,
            do_build: Box::new(|_, _, _, _| unreachable!()),
            preferences: &prefs,
        };

        // Test that we can access the apt repo
        let repo = fixer.apt_repo();
        assert_eq!(repo.directory(), td.path());
    }

    #[test]
    fn test_can_fix_no_dependency() {
        // Create a problem that can't be converted to a dependency
        #[derive(Debug)]
        struct MockProblem;

        impl std::fmt::Display for MockProblem {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "Mock Problem")
            }
        }

        impl Problem for MockProblem {
            fn kind(&self) -> std::borrow::Cow<'_, str> {
                "mock".into()
            }

            fn json(&self) -> serde_json::Value {
                serde_json::json!({
                    "type": "mock",
                    "message": "This is a mock problem"
                })
            }

            fn as_any(&self) -> &(dyn std::any::Any + 'static) {
                self
            }
        }

        let td = tempfile::tempdir().unwrap();
        let apt_repo = crate::simple_apt_repo::SimpleTrustedAptRepo::new(td.path().to_path_buf());
        let prefs = DebianizePreferences::default();

        let fixer = DebianizeFixer {
            vcs_directory: PathBuf::from("/tmp/vcs"),
            apt_repo,
            do_build: Box::new(|_, _, _, _| unreachable!()),
            preferences: &prefs,
        };

        // Test that the fixer can't fix this problem
        let problem = MockProblem;
        assert!(!fixer.can_fix(&problem));
    }

    #[test]
    fn test_extract_branch_from_url() {
        // Test fragment style
        let url = url::Url::parse("https://github.com/user/repo#branch=develop").unwrap();
        assert_eq!(extract_branch_from_url(&url), Some("develop".to_string()));

        // Test query parameter style
        let url = url::Url::parse("https://github.com/user/repo?branch=feature-123").unwrap();
        assert_eq!(
            extract_branch_from_url(&url),
            Some("feature-123".to_string())
        );

        // Test GitHub tree URL style
        let url = url::Url::parse("https://github.com/user/repo/tree/main").unwrap();
        assert_eq!(extract_branch_from_url(&url), Some("main".to_string()));

        // Test GitLab tree URL with path
        let url = url::Url::parse("https://gitlab.com/user/repo/tree/release-1.0/src").unwrap();
        assert_eq!(
            extract_branch_from_url(&url),
            Some("release-1.0".to_string())
        );

        // Test URL without branch
        let url = url::Url::parse("https://github.com/user/repo").unwrap();
        assert_eq!(extract_branch_from_url(&url), None);
    }
}
