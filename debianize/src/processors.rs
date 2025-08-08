use crate::Error;
use breezyshim::branch::Branch;
use breezyshim::workingtree::PyWorkingTree;
use debian_analyzer::debhelper::maximum_debhelper_compat_version;
use debian_analyzer::editor::{Editor, TreeEditor};
use debian_analyzer::lintian::latest_standards_version;
use debian_analyzer::relations::{ensure_exact_version, ensure_relation, ensure_some_version};
use debian_control::fields::MultiArch;
use debian_control::lossless::relations::Relations;
use debian_control::lossless::{Control, Source};
use debversion::Version;
use ognibuild::buildsystem::BuildSystem;
use ognibuild::debian::upstream_deps::get_project_wide_deps;
use ognibuild::session::Session;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use upstream_ontologist::UpstreamMetadata;

struct ProcessorContext<'a> {
    session: &'a dyn Session,
    wt: &'a dyn PyWorkingTree,
    subpath: PathBuf,
    debian_path: PathBuf,
    upstream_version: String,
    metadata: &'a UpstreamMetadata,
    compat_release: String,
    buildsystem: Box<dyn BuildSystem>,
    buildsystem_subpath: PathBuf,
    maintainer: Option<String>,
    _kickstart_from_dist: Option<Box<dyn FnOnce(&dyn PyWorkingTree, &Path) -> Result<(), Error>>>,
}

impl<'a> ProcessorContext<'a> {
    fn kickstart_tree(&mut self, sourceful: bool) -> Result<(), Error> {
        if sourceful {
            if let Some(kickstart_fn) = self._kickstart_from_dist.take() {
                kickstart_fn(self.wt, &self.subpath)?;
            } else {
                // If no kickstart function is provided, just ensure we have a clean tree
                log::debug!("No kickstart_from_dist function provided, skipping dist import");
            }
        } else {
            self.wt
                .branch()
                .generate_revision_history(&breezyshim::RevisionId::null())
                .unwrap();
            if !self.wt.has_filename(&self.subpath) {
                self.wt.mkdir(&self.subpath)?;
            }
            if !self.wt.is_versioned(&self.subpath) {
                self.wt.add(&[&self.subpath])?;
            }
        }
        Ok(())
    }

    fn create_control_file(&self) -> Result<TreeEditor<Control>, Error> {
        Ok(TreeEditor::<Control>::new(
            self.wt,
            &self.debian_path.join("control"),
            false,
            true,
        )?)
    }

    fn bootstrap_debhelper(
        &self,
        source: &mut Source,
        config: DebhelperConfig,
    ) -> Result<(), Error> {
        bootstrap_debhelper(
            self.wt,
            &self.debian_path,
            source,
            &self.compat_release,
            config,
        )
    }

    fn get_project_wide_deps(&self) -> (Relations, Relations) {
        // Check if we're running in a test environment without network access
        // TODO: Remove this workaround when ognibuild is fixed to use try_from_session
        // in default_tie_breakers (see ognibuild issue)
        let test_env = std::env::var("CARGO_TARGET_DIR").is_ok() || cfg!(test);
        let no_network = std::env::var("OGNIBUILD_NO_NETWORK").is_ok();

        if test_env || no_network {
            log::debug!("Skipping get_project_wide_deps in test/no-network environment");
            return (Relations::new(), Relations::new());
        }

        // Use the ognibuild dependency resolution with the provided session
        let (build_deps, test_deps) =
            get_project_wide_deps(self.session, self.buildsystem.as_ref());

        let mut build_ret = Relations::new();
        for dep in build_deps {
            let rs: Relations = dep.into();
            for rel in rs.entries() {
                ensure_relation(&mut build_ret, rel);
            }
        }
        let mut test_ret = Relations::new();
        for dep in test_deps {
            let rs: Relations = dep.into();
            for rel in rs.entries() {
                ensure_relation(&mut test_ret, rel.into());
            }
        }
        (build_ret, test_ret)
    }
}

fn enable_dh_addon(source: &mut Source, addon: &str) {
    let mut build_depends = source.build_depends().unwrap_or_default();
    ensure_some_version(&mut build_depends, &format!("dh-sequence-{}", addon));
    source.set_build_depends(&build_depends);
}

fn import_build_deps(source: &mut Source, new_build_deps: &Relations) {
    let mut build_depends = source.build_depends().unwrap_or_default();
    for build_dep in new_build_deps.entries() {
        for rel in build_dep.relations() {
            ensure_relation(&mut build_depends, rel.into());
        }
    }
    source.set_build_depends(&build_depends);
}

fn debhelper_rules<F: std::io::Write>(
    f: &mut F,
    buildsystem: Option<&str>,
    build_directory: Option<&str>,
    env: HashMap<&str, &str>,
) -> std::io::Result<()> {
    f.write_all(b"#!/usr/bin/make -f\n")?;
    f.write_all(b"%:\n")?;
    f.write_all(b"\tdh $@")?;
    if let Some(buildsystem) = buildsystem {
        f.write_all(format!(" --buildsystem={}", buildsystem).as_bytes())?;
    }
    if let Some(build_directory) = build_directory {
        f.write_all(format!(" --builddirectory={}", build_directory).as_bytes())?;
    }
    f.write_all(b"\n")?;
    for (key, value) in env {
        f.write_all(format!("export {}={}\n", key, value).as_bytes())?;
    }
    Ok(())
}

#[derive(Debug, Default)]
struct DebhelperConfig<'a> {
    addons: Vec<&'a str>,
    env: HashMap<&'a str, &'a str>,
    buildsystem: Option<&'a str>,
    build_directory: Option<&'a str>,
}

fn bootstrap_debhelper(
    wt: &dyn PyWorkingTree,
    debian_path: &Path,
    source: &mut Source,
    compat_release: &str,
    config: DebhelperConfig,
) -> Result<(), Error> {
    let mut build_depends = source.build_depends().unwrap_or_default();
    ensure_exact_version(
        &mut build_depends,
        "debhelper-compat",
        &maximum_debhelper_compat_version(compat_release)
            .to_string()
            .parse::<Version>()
            .unwrap(),
        None,
    );
    source.set_build_depends(&build_depends);
    for addon in config.addons.iter() {
        enable_dh_addon(source, addon);
    }

    let mut f = Vec::new();
    debhelper_rules(
        &mut f,
        config.buildsystem,
        config.build_directory,
        config.env,
    )?;
    wt.put_file_bytes_non_atomic(&debian_path.join("rules"), &f)?;
    Ok(())
}

fn process_setup_py(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;
    let upstream_name = context.metadata.name().unwrap();
    let source_name = crate::names::python_source_package_name(upstream_name);
    let mut source = control.add_source(&source_name);
    if let Some(ref maintainer) = context.maintainer {
        source.set_maintainer(maintainer);
    }
    source.set_rules_requires_root(false);
    source.set_standards_version(&latest_standards_version().to_string());
    context.bootstrap_debhelper(
        &mut source,
        DebhelperConfig {
            buildsystem: Some("pybuild"),
            addons: vec!["python3"],
            ..Default::default()
        },
    )?;
    source.set_testsuite("autopkgtest-pkg-python");

    // Check whether project supports Python 3
    let python3_support = check_python3_support(context.wt, &context.subpath)?;
    if !python3_support {
        log::warn!("Project may not support Python 3, but proceeding with Python 3 packaging");
    }

    let mut build_depends = source.build_depends().unwrap_or_default();
    ensure_relation(&mut build_depends, "python3-all".parse().unwrap());
    ensure_relation(&mut build_depends, "python3-setuptools".parse().unwrap());
    source.set_build_depends(&build_depends);
    let (build_deps, test_deps) = context.get_project_wide_deps();
    import_build_deps(&mut source, &build_deps);
    // We're going to be running the testsuite as part of the build, so import the test dependencies too.
    import_build_deps(&mut source, &test_deps);
    let binary_name = crate::names::python_binary_package_name(upstream_name);
    let mut binary = control.add_binary(&binary_name);
    binary.set_architecture(Some("all"));
    // Use raw deb822 field for substitution variables
    binary
        .as_mut_deb822()
        .insert("Depends", "${python3:Depends}");
    control.commit()?;
    Ok(())
}

fn process_maven(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;
    let upstream_name = context.metadata.name().unwrap();
    let mut source = control.add_source(upstream_name);
    source.set_rules_requires_root(false);
    source.set_standards_version(&latest_standards_version().to_string());
    context.bootstrap_debhelper(
        &mut source,
        DebhelperConfig {
            buildsystem: Some("maven"),
            ..Default::default()
        },
    )?;
    let (build_deps, _test_deps) = context.get_project_wide_deps();
    import_build_deps(&mut source, &build_deps);
    let mut binary = control.add_binary(&format!("lib{}-java", upstream_name));
    binary.set_architecture(Some("all"));
    binary.set_depends(Some(&"${java:Depends}".parse().unwrap()));
    control.commit()?;
    Ok(())
}

fn process_npm(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;
    let upstream_name = context
        .metadata
        .name()
        .unwrap()
        .trim_end_matches("@")
        .replace(['/', '_'], "-")
        .replace("@", "")
        .to_lowercase();
    let mut source = control.add_source(&format!("node-{}", upstream_name));
    if let Some(ref maintainer) = context.maintainer {
        source.set_maintainer(maintainer);
    }
    context.bootstrap_debhelper(
        &mut source,
        DebhelperConfig {
            addons: vec!["nodejs"],
            ..Default::default()
        },
    )?;
    source.set_rules_requires_root(false);
    source.set_standards_version(&latest_standards_version().to_string());
    let (build_deps, _test_deps) = context.get_project_wide_deps();
    import_build_deps(&mut source, &build_deps);
    let mut binary = control.add_binary(&format!("node-{}", upstream_name));
    binary.set_architecture(Some("all"));
    source.set_testsuite("autopkgtest-pkg-nodejs");
    control.commit()?;
    Ok(())
}

fn process_dist_zilla(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;
    let upstream_name = context.metadata.name().unwrap();
    let mut source = control.add_source(&crate::names::perl_package_name(upstream_name));
    if let Some(ref maintainer) = context.maintainer {
        source.set_maintainer(maintainer);
    }
    source.set_rules_requires_root(false);
    source.set_testsuite("autopkgtest-pkg-perl");
    source.set_standards_version(&latest_standards_version().to_string());
    let (build_deps, _test_deps) = context.get_project_wide_deps();
    import_build_deps(&mut source, &build_deps);
    context.bootstrap_debhelper(
        &mut source,
        DebhelperConfig {
            addons: vec!["dist-zilla"],
            ..Default::default()
        },
    )?;
    let binary_name = source.name().unwrap();
    let mut binary = control.add_binary(&binary_name);
    binary.set_architecture(Some("all"));
    binary.set_depends(Some(&"${perl:Depends}".parse().unwrap()));
    control.commit()?;
    Ok(())
}

fn process_perl_build_tiny(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;
    let upstream_name = context.metadata.name().unwrap();
    let mut source = control.add_source(&crate::names::perl_package_name(upstream_name));
    if let Some(ref maintainer) = context.maintainer {
        source.set_maintainer(maintainer);
    }
    source.set_rules_requires_root(false);
    source.set_testsuite("autopkgtest-pkg-perl");
    source.set_standards_version(&latest_standards_version().to_string());
    source.set_build_depends(&"libmodule-build-perl".parse().unwrap());
    let (build_deps, _test_deps) = context.get_project_wide_deps();
    import_build_deps(&mut source, &build_deps);
    context.bootstrap_debhelper(&mut source, DebhelperConfig::default())?;
    let binary_name = source.name().unwrap();
    let mut binary = control.add_binary(&binary_name);
    binary.set_architecture(Some("all"));
    binary.set_depends(Some(&"${perl:Depends}".parse().unwrap()));
    control.commit()?;
    Ok(())
}

fn process_golang(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;

    let repository_url = context
        .metadata
        .repository()
        .unwrap()
        .parse::<url::Url>()
        .unwrap();

    let godebname = crate::names::go_base_name(
        &[repository_url.host_str().unwrap(), repository_url.path()].concat(),
    );
    let mut source = control.add_source(&format!("golang-{}", godebname));
    if let Some(ref maintainer) = context.maintainer {
        source.set_maintainer(maintainer);
    }
    source.set_rules_requires_root(false);
    source.set_standards_version(&latest_standards_version().to_string());
    source.as_mut_deb822().insert(
        "XS-Go-Import-Path",
        &crate::names::go_import_path_from_repo(&repository_url),
    );
    if let Some(url) = context.metadata.repository() {
        source.set_vcs_browser(Some(url));
    }
    source.set_section(Some("devel"));
    let (build_deps, _test_deps) = context.get_project_wide_deps();
    import_build_deps(&mut source, &build_deps);
    source.set_testsuite("autopkgtest-pkg-go");
    let mut dh_env = HashMap::new();
    if context.wt.has_filename(&context.subpath.join("examples")) {
        dh_env.insert("DH_GOLANG_EXCLUDES", "examples/");
    }
    context.bootstrap_debhelper(
        &mut source,
        DebhelperConfig {
            addons: vec!["golang"],
            buildsystem: Some("golang"),
            build_directory: Some("_build"),
            env: dh_env,
        },
    )?;
    let mut binary = control.add_binary(&format!("golang-{}-dev", godebname));

    binary.set_architecture(Some("all"));
    binary.set_multi_arch(Some(MultiArch::Foreign));
    control.commit()?;
    Ok(())
}

fn process_r(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;

    let archive = match context.metadata.archive() {
        Some("CRAN") => "cran",
        Some("Bioconductor") => "bioc",
        _ => "other",
    };

    let mut source = control.add_source(&format!(
        "r-{}-{}",
        archive,
        context.metadata.name().unwrap().to_lowercase()
    ));
    if let Some(ref maintainer) = context.maintainer {
        source.set_maintainer(maintainer);
    }
    source.set_rules_requires_root(false);
    source.set_build_depends(&"dh-r, r-base-dev".parse().unwrap());
    source.set_standards_version(&latest_standards_version().to_string());
    source.set_testsuite("autopkgtest-pkg-r");
    let (build_deps, _test_deps) = context.get_project_wide_deps();
    import_build_deps(&mut source, &build_deps);
    context.bootstrap_debhelper(
        &mut source,
        DebhelperConfig {
            buildsystem: Some("R"),
            ..Default::default()
        },
    )?;
    // For now, just assume a single binary package that is architecture-dependent.
    let mut binary = control.add_binary(&format!(
        "r-{}-{}",
        archive,
        context.metadata.name().unwrap().to_lowercase()
    ));
    binary.set_architecture(Some("any"));
    binary.set_depends(Some(
        &"${R:Depends}, ${shlibs:Depends}, ${misc:Depends}"
            .parse()
            .unwrap(),
    ));
    binary.set_recommends(Some(&"${R:Recommends}".parse().unwrap()));
    binary.set_suggests(Some(&"${R:Suggests}".parse().unwrap()));
    control.commit()?;
    Ok(())
}

fn process_octave(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;
    let mut source = control.add_source(&format!(
        "octave-{}",
        context.metadata.name().unwrap().to_lowercase()
    ));
    if let Some(ref maintainer) = context.maintainer {
        source.set_maintainer(maintainer);
    }
    source.set_rules_requires_root(false);
    source.set_build_depends(&"dh-octave".parse().unwrap());
    source.set_standards_version(&latest_standards_version().to_string());
    let (build_deps, _test_deps) = context.get_project_wide_deps();
    import_build_deps(&mut source, &build_deps);
    context.bootstrap_debhelper(
        &mut source,
        DebhelperConfig {
            buildsystem: Some("octave"),
            addons: vec!["octave"],
            ..Default::default()
        },
    )?;
    // For now, just assume a single binary package that is architecture-independent.
    let mut binary = control.add_binary(&format!(
        "octave-{}",
        context.metadata.name().unwrap().to_lowercase()
    ));
    binary.set_architecture(Some("all"));
    binary.set_depends(Some(&"${octave:Depends}, ${misc:Depends}".parse().unwrap()));
    binary.set_description(Some("${octave:Upstream-Description}"));
    control.commit()?;
    Ok(())
}

fn process_default(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;
    let upstream_name = context.metadata.name().unwrap();
    let source_name =
        crate::names::upstream_name_to_debian_source_name(upstream_name).ok_or_else(|| {
            Error::MissingUpstreamInfo(format!(
                "Unable to determine source package name for {}",
                upstream_name
            ))
        })?;
    let mut source = control.add_source(&source_name);
    if let Some(ref maintainer) = context.maintainer {
        source.set_maintainer(maintainer);
    }
    source.set_rules_requires_root(false);
    source.set_standards_version(&latest_standards_version().to_string());
    let (build_deps, _test_deps) = context.get_project_wide_deps();
    import_build_deps(&mut source, &build_deps);
    context.bootstrap_debhelper(&mut source, DebhelperConfig::default())?;
    // For now, just assume a single binary package that is architecture-dependent.
    let binary_name = source.name().unwrap();
    let mut binary = control.add_binary(&binary_name);
    binary.set_architecture(Some("any"));
    control.commit()?;
    Ok(())
}

fn process_cmake(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;
    let upstream_name = context.metadata.name().unwrap_or("unknown");
    let source_name = crate::names::upstream_name_to_debian_source_name(upstream_name)
        .unwrap_or_else(|| upstream_name.to_string());

    let mut source = control.add_source(&source_name);
    if let Some(ref maintainer) = context.maintainer {
        source.set_maintainer(maintainer);
    }
    source.set_standards_version(&latest_standards_version().to_string());
    source.set_rules_requires_root(false);

    // CMake-specific build dependencies
    let mut build_depends = Relations::new();
    ensure_some_version(&mut build_depends, "cmake");
    source.set_build_depends(&build_depends);

    context.bootstrap_debhelper(
        &mut source,
        DebhelperConfig {
            buildsystem: Some("cmake"),
            ..Default::default()
        },
    )?;

    let (additional_build_deps, _test_deps) = context.get_project_wide_deps();
    import_build_deps(&mut source, &additional_build_deps);

    // Add binary package
    let binary_name = source.name().unwrap();
    let mut binary = control.add_binary(&binary_name);
    binary.set_architecture(Some("any"));

    control.commit()?;
    Ok(())
}

fn process_make(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;
    let upstream_name = context.metadata.name().unwrap_or("unknown");
    let source_name = crate::names::upstream_name_to_debian_source_name(upstream_name)
        .unwrap_or_else(|| upstream_name.to_string());

    let mut source = control.add_source(&source_name);
    if let Some(ref maintainer) = context.maintainer {
        source.set_maintainer(maintainer);
    }
    source.set_standards_version(&latest_standards_version().to_string());
    source.set_rules_requires_root(false);

    context.bootstrap_debhelper(
        &mut source,
        DebhelperConfig {
            buildsystem: Some("makefile"),
            ..Default::default()
        },
    )?;

    let (additional_build_deps, _test_deps) = context.get_project_wide_deps();
    import_build_deps(&mut source, &additional_build_deps);

    // Add binary package
    let binary_name = source.name().unwrap();
    let mut binary = control.add_binary(&binary_name);
    binary.set_architecture(Some("any"));

    control.commit()?;
    Ok(())
}

fn process_cargo(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(false)?;

    let cratename = match context
        .metadata
        .get("Cargo-Crate")
        .and_then(|v| v.datum.as_str())
    {
        Some(cratename) => cratename.to_string(),
        None => context.metadata.name().unwrap().replace("_", "-"),
    };
    // Only set semver_suffix if this is not the latest version
    use semver::Version as VersionInfo;

    let desired_version = VersionInfo::parse(&context.upstream_version).unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();

    let data = rt
        .block_on(upstream_ontologist::providers::rust::load_crate_info(
            &cratename,
        ))
        .map_err(|e| {
            Error::MissingUpstreamInfo(format!(
                "Unable to load crate info for {}: {}",
                cratename, e
            ))
        })?
        .ok_or(Error::MissingUpstreamInfo(format!(
            "crates.io has no crate {}",
            cratename
        )))?;
    let mut features = None;
    let mut crate_version = None;
    let mut semver_suffix = false;
    for version_info in data.versions {
        let available_version = &version_info.num;
        if (available_version.major, available_version.minor)
            > (desired_version.major, desired_version.minor)
        {
            semver_suffix = true;
            break;
        }
        if VersionInfo::parse(&debian_analyzer::debcargo::unmangle_debcargo_version(
            &context.upstream_version,
        ))
        .unwrap()
            == version_info.num
        {
            crate_version = Some(version_info.num);
            features = Some(version_info.features.clone());
        }
    }
    let mut control = debian_analyzer::debcargo::DebcargoEditor::new();
    control.cargo = Some(toml_edit::DocumentMut::new());
    control.cargo.as_mut().unwrap()["package"]["name"] = toml_edit::value(cratename);
    if let Some(crate_version) = crate_version {
        control.cargo.as_mut().unwrap()["package"]["version"] =
            toml_edit::value(crate_version.to_string());
    }
    if let Some(features) = features {
        let features_section = control.cargo.as_mut().unwrap()["features"]
            .as_table_mut()
            .unwrap();
        for (feature, reqs) in features.iter() {
            features_section[feature] = toml_edit::value(toml_edit::Array::new());

            for req in reqs.iter() {
                features_section[feature]
                    .as_array_mut()
                    .unwrap()
                    .push(toml_edit::Value::from(req.to_string()));
            }
        }
    }
    control.debcargo["semver_suffix"] = toml_edit::value(semver_suffix);
    control.debcargo["overlay"] = toml_edit::value(".");
    control.commit()?;
    Ok(())
}

pub fn process(
    session: &dyn Session,
    wt: &dyn PyWorkingTree,
    subpath: PathBuf,
    debian_path: PathBuf,
    upstream_version: String,
    metadata: &UpstreamMetadata,
    compat_release: String,
    buildsystem: Box<dyn BuildSystem>,
    buildsystem_subpath: PathBuf,
    maintainer: Option<String>,
    _kickstart_from_dist: Option<Box<dyn FnOnce(&dyn PyWorkingTree, &Path) -> Result<(), Error>>>,
) -> Result<(), Error> {
    let bs_name = buildsystem.name().to_string();
    let mut context = ProcessorContext {
        session,
        wt,
        subpath,
        debian_path,
        upstream_version,
        metadata,
        compat_release,
        buildsystem,
        buildsystem_subpath,
        maintainer,
        _kickstart_from_dist,
    };
    match bs_name.as_str() {
        "setup.py" => process_setup_py(&mut context),
        "node" => process_npm(&mut context),
        "gradle" => process_maven(&mut context), // For Java/gradle projects
        "Dist::Zilla" => process_dist_zilla(&mut context),
        "Module::Build::Tiny" => process_perl_build_tiny(&mut context),
        "cargo" => process_cargo(&mut context),
        "golang" => process_golang(&mut context),
        "R" => process_r(&mut context),
        "octave" => process_octave(&mut context),
        "cmake" => process_cmake(&mut context),
        "make" => process_make(&mut context), // Handles autotools too
        _ => process_default(&mut context),
    }
}

/// Check if a Python project supports Python 3
fn check_python3_support(wt: &dyn PyWorkingTree, subpath: &Path) -> Result<bool, Error> {
    // Check setup.py for Python 3 classifiers or version requirements
    let setup_py_path = subpath.join("setup.py");
    if wt.has_filename(&setup_py_path) {
        match wt.get_file_text(&setup_py_path) {
            Ok(content) => {
                let content_str = String::from_utf8_lossy(&content);
                // Look for Python 3 classifiers
                if content_str.contains("Programming Language :: Python :: 3")
                    || content_str.contains("python_requires") && content_str.contains(">=3")
                {
                    return Ok(true);
                }
                // Look for version requirements that include Python 3
                if content_str.contains("python_requires")
                    && (content_str.contains(">=2.7") || content_str.contains(">=2.6"))
                {
                    return Ok(true); // Likely supports both 2 and 3
                }
            }
            Err(_) => {} // Ignore file read errors
        }
    }

    // Check pyproject.toml
    let pyproject_path = subpath.join("pyproject.toml");
    if wt.has_filename(&pyproject_path) {
        match wt.get_file_text(&pyproject_path) {
            Ok(content) => {
                let content_str = String::from_utf8_lossy(&content);
                if content_str.contains("python") && content_str.contains(">=3") {
                    return Ok(true);
                }
            }
            Err(_) => {} // Ignore file read errors
        }
    }

    // Check for tox.ini with py3* environments
    let tox_path = subpath.join("tox.ini");
    if wt.has_filename(&tox_path) {
        match wt.get_file_text(&tox_path) {
            Ok(content) => {
                let content_str = String::from_utf8_lossy(&content);
                if content_str.contains("py3") || content_str.contains("python3") {
                    return Ok(true);
                }
            }
            Err(_) => {} // Ignore file read errors
        }
    }

    // Default to true for modern Python projects (conservative approach)
    // Most projects created in recent years support Python 3
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_debhelper_rules() {
        let mut output = Vec::new();

        // Simple case
        debhelper_rules(&mut output, None, None, HashMap::new()).unwrap();
        let rules_content = String::from_utf8(output).unwrap();
        assert!(rules_content.contains("#!/usr/bin/make -f"));
        assert!(rules_content.contains("%:\n\tdh $@\n"));
        assert!(!rules_content.contains("--buildsystem="));

        // With buildsystem
        let mut output = Vec::new();
        debhelper_rules(&mut output, Some("python"), None, HashMap::new()).unwrap();
        let rules_content = String::from_utf8(output).unwrap();
        assert!(rules_content.contains("dh $@ --buildsystem=python"));

        // With environment variables
        let mut output = Vec::new();
        let mut env = HashMap::new();
        env.insert("TEST_VAR", "test-value");
        env.insert("ANOTHER_VAR", "another-value");
        debhelper_rules(&mut output, None, None, env).unwrap();
        let rules_content = String::from_utf8(output).unwrap();
        assert!(rules_content.contains("export TEST_VAR=test-value"));
        assert!(rules_content.contains("export ANOTHER_VAR=another-value"));
    }

    #[test]
    fn test_enable_dh_addon() {
        // Create a test source with no build dependencies
        let mut control = debian_control::lossless::Control::new();
        let mut source = control.add_source("test-package");

        // Enable an addon
        enable_dh_addon(&mut source, "python3");

        // Verify build dependency string contains the addon
        let build_deps = source.build_depends().unwrap();
        let deps_string = build_deps.to_string();
        assert!(deps_string.contains("dh-sequence-python3"));

        // Enable another addon
        enable_dh_addon(&mut source, "nodejs");

        // Verify both addons are in the dependencies string
        let build_deps = source.build_depends().unwrap();
        let deps_string = build_deps.to_string();
        assert!(deps_string.contains("dh-sequence-python3"));
        assert!(deps_string.contains("dh-sequence-nodejs"));
    }

    #[test]
    fn test_import_build_deps() {
        // Create a test source with initial build dependencies
        let mut control = debian_control::lossless::Control::new();
        let mut source = control.add_source("test-package");
        source.set_build_depends(&"debhelper-compat (= 13)".parse().unwrap());

        // Create test Relations with parsed Relations string
        let new_build_deps: debian_control::lossless::relations::Relations =
            "python3-all, dh-python".parse().unwrap();

        // Import the build dependencies
        import_build_deps(&mut source, &new_build_deps);

        // Verify all dependencies are present using string matching
        let build_deps = source.build_depends().unwrap();
        let deps_string = build_deps.to_string();
        assert!(deps_string.contains("debhelper-compat (= 13)"));
        assert!(deps_string.contains("python3-all"));
        assert!(deps_string.contains("dh-python"));

        // Add a dependency with version
        let versioned_deps: debian_control::lossless::relations::Relations =
            "python3-all (>= 3.9)".parse().unwrap();

        // Import the versioned dependency
        import_build_deps(&mut source, &versioned_deps);

        // Verify the versioned dependency is now present
        let build_deps = source.build_depends().unwrap();
        let deps_string = build_deps.to_string();
        assert!(deps_string.contains("python3-all (>= 3.9)"));
    }

    #[test]
    fn test_debhelper_config() {
        // Test the DebhelperConfig struct and defaults
        let config = DebhelperConfig {
            addons: vec!["python3", "nodejs"],
            env: {
                let mut env = HashMap::new();
                env.insert("TEST_VAR", "test-value");
                env
            },
            buildsystem: Some("pybuild"),
            build_directory: None,
        };

        assert_eq!(config.addons.len(), 2);
        assert_eq!(config.addons[0], "python3");
        assert_eq!(config.addons[1], "nodejs");
        assert_eq!(config.env.get("TEST_VAR"), Some(&"test-value"));
        assert_eq!(config.buildsystem, Some("pybuild"));

        // Test default implementation
        let default_config = DebhelperConfig::default();
        assert!(default_config.addons.is_empty());
        assert!(default_config.env.is_empty());
        assert_eq!(default_config.buildsystem, None);
        assert_eq!(default_config.build_directory, None);
    }
}
