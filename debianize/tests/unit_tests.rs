use debianize::DebianizePreferences;

#[test]
fn test_python_package_name_generation() {
    // Test Python source package name generation
    assert_eq!(
        debianize::names::python_source_package_name("hello-world"),
        "python-hello-world"
    );
    assert_eq!(
        debianize::names::python_source_package_name("MyPackage"),
        "python-mypackage"
    );
    assert_eq!(
        debianize::names::python_source_package_name("test_pkg"),
        "python-test-pkg"
    );
}

#[test]
fn test_python_binary_package_name_generation() {
    // Test Python binary package name generation
    assert_eq!(
        debianize::names::python_binary_package_name("hello-world"),
        "python3-hello-world"
    );
    assert_eq!(
        debianize::names::python_binary_package_name("MyPackage"),
        "python3-mypackage"
    );
}

#[test]
fn test_preferences_defaults() {
    let prefs = DebianizePreferences {
        use_inotify: None,
        diligence: 0,
        trust: false,
        check: false,
        net_access: true,
        force_subprocess: false,
        force_new_directory: false,
        compat_release: None,
        minimum_certainty: debian_analyzer::Certainty::Confident,
        consult_external_directory: false,
        verbose: false,
        session: debianize::SessionPreferences::Plain,
        create_dist: None,
        committer: None,
        upstream_version_kind: breezyshim::debian::VersionKind::Auto,
        debian_revision: "1".to_string(),
        team: None,
        author: None,
        compat_level: None,
        check_wnpp: true,
        run_fixers: true,
    };

    // Basic sanity checks
    assert_eq!(prefs.debian_revision, "1");
    assert_eq!(
        prefs.upstream_version_kind,
        breezyshim::debian::VersionKind::Auto
    );
    assert!(!prefs.trust);
}

#[test]
fn test_extract_branch_from_url() {
    use debianize::fixer::extract_branch_from_url;
    use url::Url;

    // Test various URL patterns
    assert_eq!(
        extract_branch_from_url(
            &Url::parse("https://github.com/user/repo.git#branch=develop").unwrap()
        ),
        Some("develop".to_string())
    );

    assert_eq!(
        extract_branch_from_url(
            &Url::parse("https://github.com/user/repo.git?branch=feature/test").unwrap()
        ),
        Some("feature/test".to_string())
    );

    assert_eq!(
        extract_branch_from_url(&Url::parse("https://github.com/user/repo/tree/main").unwrap()),
        Some("main".to_string())
    );

    assert_eq!(
        extract_branch_from_url(&Url::parse("https://github.com/user/repo.git").unwrap()),
        None
    );
}

#[test]
fn test_perl_package_name_generation() {
    assert_eq!(
        debianize::names::perl_package_name("Test-Module"),
        "libtest-module-perl"
    );
    assert_eq!(
        debianize::names::perl_package_name("My::Module"),
        "libmy-module-perl"
    );
}

#[test]
fn test_go_package_name_generation() {
    assert_eq!(
        debianize::names::go_base_name("github.com/user/project"),
        "github-user-project"
    );
    assert_eq!(
        debianize::names::go_base_name("golang.org/x/tools"),
        "golang-x-tools"
    );
}

#[test]
fn test_go_import_path_from_repo() {
    let url = url::Url::parse("https://github.com/user/project").unwrap();
    assert_eq!(
        debianize::names::go_import_path_from_repo(&url),
        "github.com/user/project"
    );

    let url = url::Url::parse("https://gitlab.com/group/subgroup/project").unwrap();
    assert_eq!(
        debianize::names::go_import_path_from_repo(&url),
        "gitlab.com/group/subgroup/project"
    );
}

#[test]
fn test_upstream_name_to_debian_source_name() {
    // Test various upstream names
    assert_eq!(
        debianize::names::upstream_name_to_debian_source_name("hello-world"),
        Some("hello-world".to_string())
    );

    assert_eq!(
        debianize::names::upstream_name_to_debian_source_name("MyApp"),
        Some("myapp".to_string())
    );

    assert_eq!(
        debianize::names::upstream_name_to_debian_source_name("test_project"),
        Some("test-project".to_string())
    );

    // Test with special characters
    assert_eq!(
        debianize::names::upstream_name_to_debian_source_name("My.App"),
        Some("my.app".to_string())
    );
}
