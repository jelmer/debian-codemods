use crate::Error;
use breezyshim::tree::{MutableTree, Tree};
use breezyshim::workingtree::WorkingTree;
use debian_analyzer::debhelper::maximum_debhelper_compat_version;
use debian_analyzer::editor::{Editor, TreeEditor};
use debian_analyzer::lintian::latest_standards_version;
use debian_analyzer::relations::{ensure_exact_version, ensure_relation, ensure_some_version};
use debian_control::fields::MultiArch;
use debian_control::lossless::relations::Relations;
use debian_control::lossless::{Binary, Control, Source};
use debversion::Version;
use ognibuild::buildsystem::BuildSystem;
use ognibuild::debian::upstream_deps::get_project_wide_deps;
use ognibuild::session::Session;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use upstream_ontologist::UpstreamMetadata;

struct ProcessorContext {
    session: Box<dyn Session>,
    wt: WorkingTree,
    subpath: PathBuf,
    debian_path: PathBuf,
    upstream_version: String,
    metadata: UpstreamMetadata,
    compat_release: String,
    buildsystem: Box<dyn BuildSystem>,
    buildsystem_subpath: PathBuf,
    _kickstart_from_dist: Option<Box<dyn FnOnce(&WorkingTree, &Path) -> Result<(), Error>>>,
}

impl ProcessorContext {
    fn kickstart_tree(&mut self, sourceful: bool) -> Result<(), Error> {
        if sourceful {
            (self._kickstart_from_dist.take().unwrap())(&self.wt, &self.subpath)?;
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
            &self.wt,
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
            &self.wt,
            &self.debian_path,
            source,
            &self.compat_release,
            config,
        )
    }

    fn get_project_wide_deps(&self) -> (Relations, Relations) {
        let (build_deps, test_deps) =
            get_project_wide_deps(self.session.as_ref(), self.buildsystem.as_ref());
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
    env: HashMap<&str, &str>,
) -> std::io::Result<()> {
    f.write_all(b"#!/usr/bin/make -f\n")?;
    f.write_all(b"%:\n")?;
    f.write_all(b"\tdh $@")?;
    if let Some(buildsystem) = buildsystem {
        f.write_all(format!(" --buildsystem={}", buildsystem).as_bytes())?;
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
}

fn bootstrap_debhelper(
    wt: &WorkingTree,
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
    debhelper_rules(&mut f, config.buildsystem, config.env)?;
    wt.put_file_bytes_non_atomic(&debian_path.join("rules"), &f)?;
    Ok(())
}

fn process_setup_py(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;
    let upstream_name = context.metadata.name().unwrap();
    let source_name = crate::names::python_source_package_name(upstream_name);
    let mut source = control.add_source(&source_name);
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
    // TODO(jelmer): check whether project supports python 3
    let mut build_depends = source.build_depends().unwrap_or_default();
    ensure_relation(&mut build_depends, "python3-all".parse().unwrap());
    source.set_build_depends(&build_depends);
    let (build_deps, test_deps) = context.get_project_wide_deps();
    import_build_deps(&mut source, &build_deps);
    // We're going to be running the testsuite as part of the build, so import the test dependencies too.
    import_build_deps(&mut source, &test_deps);
    let binary_name = crate::names::python_binary_package_name(upstream_name);
    let mut binary = control.add_binary(&binary_name);
    binary.set_architecture(Some("all"));
    binary.set_depends(Some(&"${python3:Depends}".parse().unwrap()));
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

fn process_makefile_pl(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;
    let upstream_name = context.metadata.name().unwrap();
    let mut source = control.add_source(&crate::names::perl_package_name(upstream_name));
    source.set_rules_requires_root(false);
    source.set_testsuite("autopkgtest-pkg-perl");
    source.set_standards_version(&latest_standards_version().to_string());
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

fn process_perl_build_tiny(context: &mut ProcessorContext) -> Result<(), Error> {
    context.kickstart_tree(true)?;
    let mut control = context.create_control_file()?;
    let upstream_name = context.metadata.name().unwrap();
    let mut source = control.add_source(&crate::names::perl_package_name(upstream_name));
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
            env: dh_env,
        },
    )?;
    // TODO(jelmer): Add --builddirectory=_build to dh arguments
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

    let data = upstream_ontologist::providers::rust::load_crate_info(&cratename)
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
    session: Box<dyn Session>,
    wt: WorkingTree,
    subpath: PathBuf,
    debian_path: PathBuf,
    upstream_version: String,
    metadata: UpstreamMetadata,
    compat_release: String,
    buildsystem: Box<dyn BuildSystem>,
    buildsystem_subpath: PathBuf,
    _kickstart_from_dist: Option<Box<dyn FnOnce(&WorkingTree, &Path) -> Result<(), Error>>>,
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
        _kickstart_from_dist,
    };
    match bs_name.as_str() {
        "setup.py" => process_setup_py(&mut context),
        "npm" => process_npm(&mut context),
        "maven" => process_maven(&mut context),
        "dist-zilla" => process_dist_zilla(&mut context),
        "dist-inkt" => process_dist_zilla(&mut context),
        "perl-build-tiny" => process_perl_build_tiny(&mut context),
        "makefile.pl" => process_makefile_pl(&mut context),
        "cargo" => process_cargo(&mut context),
        "golang" => process_golang(&mut context),
        "R" => process_r(&mut context),
        "octave" => process_octave(&mut context),
        _ => process_default(&mut context),
    }
}
