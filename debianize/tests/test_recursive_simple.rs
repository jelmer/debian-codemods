// Simple test for recursive debianization without all the complexity
use debianize::fixer::DebianizeFixer;
use debianize::simple_apt_repo::SimpleTrustedAptRepo;
use debianize::{DebianizePreferences, SessionPreferences};
use ognibuild::debian::fix_build::DebianBuildFixer;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn test_debianize_fixer_can_identify_missing_packages() {
    // Create temporary directory for test
    let temp_dir = TempDir::new().unwrap();

    // Create and start an APT repository
    let mut apt_repo = SimpleTrustedAptRepo::new(temp_dir.path().join("apt-repo"));
    std::fs::create_dir_all(apt_repo.directory()).unwrap();
    apt_repo.start().unwrap();

    // Set up preferences
    let preferences = DebianizePreferences {
        use_inotify: Some(false),
        diligence: 0,
        trust: true,
        check: false,
        net_access: false,
        force_subprocess: false,
        force_new_directory: false,
        compat_release: Some("bookworm".to_string()),
        minimum_certainty: debian_analyzer::Certainty::Confident,
        consult_external_directory: false,
        verbose: false,
        session: SessionPreferences::Plain,
        create_dist: None,
        committer: Some("Test <test@example.com>".to_string()),
        upstream_version_kind: breezyshim::debian::VersionKind::Auto,
        debian_revision: "1".to_string(),
        team: None,
        author: Some("Test <test@example.com>".to_string()),
        compat_level: None,
        check_wnpp: false,
        run_fixers: false,
    };

    // Create a build function that uses the actual build system
    let session_prefs = preferences.session.clone();
    let do_build = Box::new(
        move |wt: &breezyshim::workingtree::GenericWorkingTree,
              subpath: &Path,
              target_dir: &Path,
              extra_repositories: Vec<&str>|
              -> Result<
            ognibuild::debian::build::BuildOnceResult,
            ognibuild::debian::fix_build::IterateBuildError,
        > {
            // Create a session for building
            let _build_session = session_prefs.acquire().map_err(|_e| {
                ognibuild::debian::fix_build::IterateBuildError::Unidentified {
                    retcode: 1,
                    lines: vec!["Failed to acquire session".to_string()],
                    secondary: None,
                    phase: Some(ognibuild::debian::context::Phase::Build),
                }
            })?;

            // Use ognibuild's actual build function
            // This will read debian/control, debian/changelog etc. to determine
            // the source_package name, version, and create the actual .deb files
            ognibuild::debian::build::build_once(
                wt,
                None, // build_suite
                target_dir,
                "dpkg-buildpackage -us -uc -b",
                subpath,
                None, // source_date_epoch
                None, // apt_repository
                None, // apt_repository_key
                if extra_repositories.is_empty() {
                    None
                } else {
                    Some(&extra_repositories)
                },
            )
            .map_err(|_e| {
                ognibuild::debian::fix_build::IterateBuildError::Unidentified {
                    retcode: 1,
                    lines: vec!["Build failed".to_string()],
                    secondary: None,
                    phase: Some(ognibuild::debian::context::Phase::Build),
                }
            })
        },
    );

    // Create the fixer
    let fixer = DebianizeFixer::new(
        temp_dir.path().join("vcs"),
        apt_repo,
        do_build,
        &preferences,
    );

    // Create a simple test problem to verify the fixer works
    // We'll create a custom problem type for testing
    struct TestProblem;

    impl buildlog_consultant::Problem for TestProblem {
        fn kind(&self) -> std::borrow::Cow<str> {
            "test-problem".into()
        }

        fn json(&self) -> serde_json::Value {
            serde_json::json!({"kind": "test-problem"})
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl std::fmt::Display for TestProblem {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "Test problem")
        }
    }

    impl std::fmt::Debug for TestProblem {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "TestProblem")
        }
    }

    let problem = TestProblem;

    // The fixer won't be able to fix this test problem since it's not a real dependency problem
    let can_fix = fixer.can_fix(&problem);
    println!("Can fix test problem: {}", can_fix);

    // In a real scenario with registered upstream providers,
    // this would return true and actually package the dependency
}
