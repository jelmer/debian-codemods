/// Integration tests for debianization
mod common;

use breezyshim::testing::TestEnv;
use breezyshim::workingtree::WorkingTree;
use common::*;
use debian_analyzer::lintian::latest_standards_version;
use debianize::debianize;
use serial_test::serial;
use std::path::Path;
use tempfile::TempDir;
use upstream_ontologist::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata, UpstreamMetadata};

#[test]
#[serial]
fn test_debianize_simple_python_package() {
    let image_cached = match DebianImageCached::new() {
        Ok(cached) => cached,
        Err(e) => {
            eprintln!("Failed to cache Debian image: {:?}", e);
            return;
        }
    };
    let test_env = TestEnv::new();

    let temp_dir = TempDir::new().unwrap();
    let (repo_path, wt) = create_test_python_repo(&temp_dir, "hello-world");

    // Create package with dependencies
    create_simple_python_package(&wt, "hello-world", "0.1.0", &["requests>=2.25.0"]);

    // Run debianize
    let preferences = default_test_preferences();
    let metadata = UpstreamMetadata::new();

    let result = debianize(
        &wt,
        Path::new(""),
        Some(&wt.branch()),
        Some(Path::new("")),
        &preferences,
        Some("0.1.0"),
        &metadata,
    );

    assert!(result.is_ok(), "Debianize failed: {:?}", result.err());

    // Verify debian files exist
    assert_debian_files_exist(&wt);

    // Check debian/control content
    let control_content = read_cleaned_control(&repo_path);
    let expected_control = format!(
        "Source: python-hello-world\n\
         Section: python\n\
         Maintainer: Test Packager <packager@example.com>\n\
         Build-Depends: debhelper-compat (= 13), dh-sequence-python3, python3-all, python3-setuptools\n\
         Standards-Version: {}\n\
         Rules-Requires-Root: no\n\
         Testsuite: autopkgtest-pkg-python\n\
         \n\
         Package: python3-hello-world\n\
         Architecture: all\n\
         Depends: ${{python3:Depends}}\n",
        latest_standards_version()
    );
    assert_eq!(control_content, expected_control);

    // Check debian/rules content
    let rules_path = repo_path.join("debian/rules");
    let rules_content = std::fs::read_to_string(&rules_path).unwrap();
    let expected_rules = "#!/usr/bin/make -f\n%:\n\tdh $@ --buildsystem=pybuild\n";
    assert_eq!(rules_content, expected_rules);

    // Check debian/source/format
    let format_path = repo_path.join("debian/source/format");
    let format_content = std::fs::read_to_string(&format_path).unwrap();
    assert_eq!(format_content, "3.0 (quilt)\n");

    std::mem::drop(image_cached);
    std::mem::drop(test_env);
}

#[test]
#[serial]
fn test_debianize_pyproject_package() {
    use breezyshim::tree::MutableTree;

    let image_cached = match DebianImageCached::new() {
        Ok(cached) => cached,
        Err(e) => {
            eprintln!("Failed to cache Debian image: {:?}", e);
            return;
        }
    };
    let test_env = TestEnv::new();

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().join("hello-toml");
    std::fs::create_dir_all(&repo_path).unwrap();
    let wt = init_working_tree(&repo_path);

    wt.put_file_bytes_non_atomic(
        Path::new("pyproject.toml"),
        b"[build-system]\nrequires = [\"setuptools\"]\nbuild-backend = \"setuptools.build_meta\"\n\n[project]\nname = \"hello-toml\"\nversion = \"0.1.0\"\ndescription = \"Test package\"\n",
    )
    .unwrap();
    wt.put_file_bytes_non_atomic(Path::new("hello_toml.py"), b"__version__ = \"0.1.0\"\n")
        .unwrap();
    wt.add(&[Path::new("pyproject.toml"), Path::new("hello_toml.py")])
        .unwrap();
    wt.build_commit()
        .message("Initial commit")
        .commit()
        .unwrap();

    let preferences = default_test_preferences();
    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("hello-toml".to_string()),
        certainty: Some(Certainty::Certain),
        origin: None,
    });

    let result = debianize(
        &wt,
        Path::new(""),
        Some(&wt.branch()),
        Some(Path::new("")),
        &preferences,
        Some("0.1.0"),
        &metadata,
    );

    assert!(result.is_ok(), "Debianize failed: {:?}", result.err());

    // pybuild needs the pyproject plugin for PEP 517 backends; without it
    // the build fails at dh_auto_configure.
    let control_content = read_cleaned_control(&repo_path);
    let expected_control = format!(
        "Source: python-hello-toml\n\
         Section: python\n\
         Maintainer: Test Packager <packager@example.com>\n\
         Build-Depends: debhelper-compat (= 13), dh-sequence-python3, pybuild-plugin-pyproject, python3-all, python3-setuptools\n\
         Standards-Version: {}\n\
         Rules-Requires-Root: no\n\
         Testsuite: autopkgtest-pkg-python\n\
         \n\
         Package: python3-hello-toml\n\
         Architecture: all\n\
         Depends: ${{python3:Depends}}\n",
        latest_standards_version()
    );
    assert_eq!(control_content, expected_control);

    std::mem::drop(image_cached);
    std::mem::drop(test_env);
}

#[test]
#[serial]
fn test_debianize_with_custom_maintainer() {
    let image_cached = match DebianImageCached::new() {
        Ok(cached) => cached,
        Err(e) => {
            eprintln!("Failed to cache Debian image: {:?}", e);
            return;
        }
    };
    let test_env = TestEnv::new();
    let temp_dir = TempDir::new().unwrap();
    let (repo_path, wt) = create_test_python_repo(&temp_dir, "test-pkg");

    create_simple_python_package(&wt, "test-pkg", "1.0.0", &[]);

    // Custom preferences with different maintainer
    let mut preferences = default_test_preferences();
    preferences.author = Some("John Doe <john@example.com>".to_string());

    let metadata = UpstreamMetadata::new();
    let result = debianize(
        &wt,
        Path::new(""),
        Some(&wt.branch()),
        Some(Path::new("")),
        &preferences,
        Some("1.0.0"),
        &metadata,
    );

    assert!(result.is_ok(), "Debianize failed: {:?}", result.err());

    // Verify custom maintainer in control file
    let control_content = read_cleaned_control(&repo_path);
    assert!(control_content.contains("Maintainer: John Doe <john@example.com>"));
    std::mem::drop(test_env);
    std::mem::drop(image_cached);
}

/// Create a simple Module::Build::Tiny Perl package in the working tree
fn create_perl_build_tiny_package(wt: &breezyshim::workingtree::GenericWorkingTree, name: &str) {
    use breezyshim::tree::MutableTree;

    wt.put_file_bytes_non_atomic(
        Path::new("Build.PL"),
        b"use Module::Build::Tiny;\nBuild_PL();\n",
    )
    .unwrap();

    let module_path = format!("lib/{}.pm", name);
    wt.mkdir(Path::new("lib")).unwrap();
    wt.put_file_bytes_non_atomic(
        Path::new(&module_path),
        format!("package {};\n1;\n", name.replace('-', "::")).as_bytes(),
    )
    .unwrap();

    wt.add(&[
        Path::new("Build.PL"),
        Path::new("lib"),
        Path::new(&module_path),
    ])
    .unwrap();

    wt.build_commit()
        .message(&format!("Initial commit for {}", name))
        .commit()
        .unwrap();
}

#[test]
#[serial]
fn test_debianize_autotools_package() {
    use breezyshim::tree::MutableTree;

    let image_cached = match DebianImageCached::new() {
        Ok(cached) => cached,
        Err(e) => {
            eprintln!("Failed to cache Debian image: {:?}", e);
            return;
        }
    };
    let test_env = TestEnv::new();

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().join("hello");
    std::fs::create_dir_all(&repo_path).unwrap();
    let wt = init_working_tree(&repo_path);

    wt.put_file_bytes_non_atomic(
        Path::new("configure.ac"),
        b"AC_INIT([hello], [0.1.0])\nAM_INIT_AUTOMAKE([foreign])\nAC_PROG_CC\nAC_CONFIG_FILES([Makefile])\nAC_OUTPUT\n",
    )
    .unwrap();
    wt.put_file_bytes_non_atomic(
        Path::new("Makefile.am"),
        b"bin_PROGRAMS = hello\nhello_SOURCES = hello.c\n",
    )
    .unwrap();
    wt.put_file_bytes_non_atomic(Path::new("hello.c"), b"int main(void) { return 0; }\n")
        .unwrap();
    wt.add(&[
        Path::new("configure.ac"),
        Path::new("Makefile.am"),
        Path::new("hello.c"),
    ])
    .unwrap();
    wt.build_commit()
        .message("Initial commit")
        .commit()
        .unwrap();

    let preferences = default_test_preferences();
    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("hello".to_string()),
        certainty: Some(Certainty::Certain),
        origin: None,
    });

    let result = debianize(
        &wt,
        Path::new(""),
        Some(&wt.branch()),
        Some(Path::new("")),
        &preferences,
        Some("0.1.0"),
        &metadata,
    );

    assert!(result.is_ok(), "Debianize failed: {:?}", result.err());

    // dh autodetects autoconf; forcing the makefile buildsystem would skip
    // running configure entirely.
    let rules_content = std::fs::read_to_string(repo_path.join("debian/rules")).unwrap();
    assert_eq!(
        rules_content,
        "#!/usr/bin/make -f\n%:\n\tdh $@ --buildsystem=makefile\n"
    );

    std::mem::drop(image_cached);
    std::mem::drop(test_env);
}

#[test]
#[serial]
fn test_debianize_maven_package() {
    use breezyshim::tree::MutableTree;

    let image_cached = match DebianImageCached::new() {
        Ok(cached) => cached,
        Err(e) => {
            eprintln!("Failed to cache Debian image: {:?}", e);
            return;
        }
    };
    let test_env = TestEnv::new();

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().join("hello-java");
    std::fs::create_dir_all(&repo_path).unwrap();
    let wt = init_working_tree(&repo_path);

    wt.put_file_bytes_non_atomic(
        Path::new("pom.xml"),
        b"<project>\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>com.example</groupId>\n  <artifactId>hello-java</artifactId>\n  <version>0.1.0</version>\n  <name>Hello Java</name>\n</project>\n",
    )
    .unwrap();
    wt.add(&[Path::new("pom.xml")]).unwrap();
    wt.build_commit()
        .message("Initial commit")
        .commit()
        .unwrap();

    let preferences = default_test_preferences();
    let mut metadata = UpstreamMetadata::new();
    // The pom.xml name is a human-readable string, not a package name.
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("Hello Java".to_string()),
        certainty: Some(Certainty::Certain),
        origin: None,
    });

    let result = debianize(
        &wt,
        Path::new(""),
        Some(&wt.branch()),
        Some(Path::new("")),
        &preferences,
        Some("0.1.0"),
        &metadata,
    );

    assert!(result.is_ok(), "Debianize failed: {:?}", result.err());

    assert_debian_files_exist(&wt);

    let control_content = read_cleaned_control(&repo_path);
    let expected_control = format!(
        "Source: Hello Java\n\
         Section: java\n\
         Build-Depends: debhelper-compat (= 13)\n\
         Standards-Version: {}\n\
         Rules-Requires-Root: no\n\
         \n\
         Package: libHello Java-java\n\
         Architecture: all\n\
         Depends: ${{java:Depends}}\n",
        latest_standards_version()
    );
    assert_eq!(control_content, expected_control);

    let rules_content = std::fs::read_to_string(repo_path.join("debian/rules")).unwrap();
    assert_eq!(
        rules_content,
        "#!/usr/bin/make -f\n%:\n\tdh $@ --buildsystem=maven\n"
    );

    std::mem::drop(image_cached);
    std::mem::drop(test_env);
}

#[test]
#[serial]
fn test_debianize_make_package() {
    use breezyshim::tree::MutableTree;

    let image_cached = match DebianImageCached::new() {
        Ok(cached) => cached,
        Err(e) => {
            eprintln!("Failed to cache Debian image: {:?}", e);
            return;
        }
    };
    let test_env = TestEnv::new();

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().join("hello");
    std::fs::create_dir_all(&repo_path).unwrap();
    let wt = init_working_tree(&repo_path);

    wt.put_file_bytes_non_atomic(
        Path::new("Makefile"),
        b"all: hello\n\nhello: hello.c\n\tcc -o hello hello.c\n\ninstall:\n\tinstall -D hello $(DESTDIR)/usr/bin/hello\n",
    )
    .unwrap();
    wt.put_file_bytes_non_atomic(Path::new("hello.c"), b"int main(void) { return 0; }\n")
        .unwrap();
    wt.add(&[Path::new("Makefile"), Path::new("hello.c")])
        .unwrap();
    wt.build_commit()
        .message("Initial commit")
        .commit()
        .unwrap();

    let mut preferences = default_test_preferences();
    preferences.gbp = true;
    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("hello".to_string()),
        certainty: Some(Certainty::Certain),
        origin: None,
    });

    let result = debianize(
        &wt,
        Path::new(""),
        Some(&wt.branch()),
        Some(Path::new("")),
        &preferences,
        Some("0.1.0"),
        &metadata,
    );

    assert!(result.is_ok(), "Debianize failed: {:?}", result.err());

    assert_debian_files_exist(&wt);

    let control_content = read_cleaned_control(&repo_path);
    let expected_control = format!(
        "Source: hello\n\
         Section: misc\n\
         Maintainer: Test Packager <packager@example.com>\n\
         Build-Depends: debhelper-compat (= 13)\n\
         Standards-Version: {}\n\
         Rules-Requires-Root: no\n\
         \n\
         Package: hello\n\
         Architecture: any\n\
         Depends: ${{misc:Depends}}, ${{shlibs:Depends}}\n",
        latest_standards_version()
    );
    assert_eq!(control_content, expected_control);

    let rules_content = std::fs::read_to_string(repo_path.join("debian/rules")).unwrap();
    assert_eq!(
        rules_content,
        "#!/usr/bin/make -f\n%:\n\tdh $@ --buildsystem=makefile\n"
    );

    // The test tree's git branch has no name, so no debian-branch entry.
    let gbp_conf = std::fs::read_to_string(repo_path.join("debian/gbp.conf")).unwrap();
    assert_eq!(gbp_conf, "[DEFAULT]\nupstream-branch = upstream\n");

    std::mem::drop(image_cached);
    std::mem::drop(test_env);
}

/// Create a simple ExtUtils::MakeMaker Perl package in the working tree
fn create_makefile_pl_package(wt: &breezyshim::workingtree::GenericWorkingTree, name: &str) {
    use breezyshim::tree::MutableTree;

    let module = name.replace('-', "::");
    wt.put_file_bytes_non_atomic(
        Path::new("Makefile.PL"),
        format!(
            "use ExtUtils::MakeMaker;\nWriteMakefile(NAME => '{}', VERSION => '0.1.0');\n",
            module
        )
        .as_bytes(),
    )
    .unwrap();

    let module_path = format!("lib/{}.pm", name);
    wt.mkdir(Path::new("lib")).unwrap();
    wt.put_file_bytes_non_atomic(
        Path::new(&module_path),
        format!("package {};\n1;\n", module).as_bytes(),
    )
    .unwrap();

    wt.add(&[
        Path::new("Makefile.PL"),
        Path::new("lib"),
        Path::new(&module_path),
    ])
    .unwrap();

    wt.build_commit()
        .message(&format!("Initial commit for {}", name))
        .commit()
        .unwrap();
}

#[test]
#[serial]
fn test_debianize_makefile_pl_package() {
    let image_cached = match DebianImageCached::new() {
        Ok(cached) => cached,
        Err(e) => {
            eprintln!("Failed to cache Debian image: {:?}", e);
            return;
        }
    };
    let test_env = TestEnv::new();

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().join("Foo-Bar");
    std::fs::create_dir_all(&repo_path).unwrap();
    let wt = init_working_tree(&repo_path);

    create_makefile_pl_package(&wt, "Foo-Bar");

    let preferences = default_test_preferences();
    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("Foo-Bar".to_string()),
        certainty: Some(Certainty::Certain),
        origin: None,
    });

    let result = debianize(
        &wt,
        Path::new(""),
        Some(&wt.branch()),
        Some(Path::new("")),
        &preferences,
        Some("0.1.0"),
        &metadata,
    );

    assert!(result.is_ok(), "Debianize failed: {:?}", result.err());

    assert_debian_files_exist(&wt);

    let control_content = read_cleaned_control(&repo_path);
    let expected_control = format!(
        "Source: libfoo-bar-perl\n\
         Section: perl\n\
         Maintainer: Test Packager <packager@example.com>\n\
         Build-Depends: debhelper-compat (= 13)\n\
         Standards-Version: {}\n\
         Rules-Requires-Root: no\n\
         Testsuite: autopkgtest-pkg-perl\n\
         \n\
         Package: libfoo-bar-perl\n\
         Architecture: all\n\
         Depends: ${{perl:Depends}}\n",
        latest_standards_version()
    );
    assert_eq!(control_content, expected_control);

    // dh autodetects perl_makemaker from Makefile.PL; forcing the makefile
    // buildsystem would skip running Makefile.PL entirely.
    let rules_content = std::fs::read_to_string(repo_path.join("debian/rules")).unwrap();
    assert_eq!(rules_content, "#!/usr/bin/make -f\n%:\n\tdh $@\n");

    std::mem::drop(image_cached);
    std::mem::drop(test_env);
}

#[test]
#[serial]
fn test_debianize_perl_build_tiny_package() {
    let image_cached = match DebianImageCached::new() {
        Ok(cached) => cached,
        Err(e) => {
            eprintln!("Failed to cache Debian image: {:?}", e);
            return;
        }
    };
    let test_env = TestEnv::new();

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().join("Foo-Bar");
    std::fs::create_dir_all(&repo_path).unwrap();
    let wt = init_working_tree(&repo_path);

    create_perl_build_tiny_package(&wt, "Foo-Bar");

    let preferences = default_test_preferences();
    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("Foo-Bar".to_string()),
        certainty: Some(Certainty::Certain),
        origin: None,
    });

    let result = debianize(
        &wt,
        Path::new(""),
        Some(&wt.branch()),
        Some(Path::new("")),
        &preferences,
        Some("0.1.0"),
        &metadata,
    );

    assert!(result.is_ok(), "Debianize failed: {:?}", result.err());

    assert_debian_files_exist(&wt);

    // The Depends field contains substitution variables, which must be set
    // verbatim rather than parsed as relations.
    let control_content = read_cleaned_control(&repo_path);
    let expected_control = format!(
        "Source: libfoo-bar-perl\n\
         Section: perl\n\
         Maintainer: Test Packager <packager@example.com>\n\
         Build-Depends: debhelper-compat (= 13), libmodule-build-perl\n\
         Standards-Version: {}\n\
         Rules-Requires-Root: no\n\
         Testsuite: autopkgtest-pkg-perl\n\
         \n\
         Package: libfoo-bar-perl\n\
         Architecture: all\n\
         Depends: ${{perl:Depends}}\n",
        latest_standards_version()
    );
    assert_eq!(control_content, expected_control);

    std::mem::drop(image_cached);
    std::mem::drop(test_env);
}
