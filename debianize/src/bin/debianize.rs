use clap::Parser;
use log::warn;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;
use debversion::Version;
use breezyshim::tree::Tree;

use std::io::Write as _;
use std::path::{PathBuf, Path};

use breezyshim::debian::VersionKind;

use ognibuild::dependencies::debian::DebianDependency;
use upstream_ontologist::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata};
use debian_analyzer::svp::{self,report_fatal, report_nothing_to_do, report_success_debian};
use debianize::simple_apt_repo::SimpleTrustedAptRepo;

/// Create Debian packaging for upstream projects, in version control
#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    /// Debug mode
    #[clap(long)]
    debug: bool,

    /// Directory to run in
    #[clap(long, default_value = ".")]
    directory: PathBuf,

    #[clap(long)]
    disable_inotify: bool,

    #[arg(long, env = "COMPAT_RELEASE", value_name = "RELEASE", hide = true)]
    compat_release: Option<String>,

    /// Be verbose
    #[arg(long)]
    verbose: bool,

    /// Do not probe external services
    #[arg(long)]
    disable_net_access: bool,

    #[arg(long, hide = true, default_value_t = 0)]
    diligence: u8,

    /// Whether to allow running code from the package
    #[arg(long)]
    trust: bool,

    /// Pull in external (not maintained by upstream) directory data
    #[arg(long)]
    consult_external_directory: bool,

    /// Check guessed metadata against external sources
    #[arg(long)]
    check: bool,

    #[arg(long, hide = true)]
    force_subprocess: bool,

    /// Create a new debian/ directory even if one already exists
    #[arg(long)]
    force_new_directory: bool,

    /// Invoke deb-fix-build afterwards to build package and add missing dependencies
    #[arg(long, short('x'))]
    iterate_fix: bool,

    /// Install package after building (implies --iterate-fix)
    #[arg(long, short('i'))]
    install: bool,

    /// Schroot to use for building apt archive access
    #[arg(long, env = "SCHROOT")]
    schroot: Option<String>,

    /// Unshare tarball to use for building apt archive access
    #[arg(long)]
    unshare: Option<String>,

    /// Build command (used for --iterate-fix)
    #[arg(long, default_value_t = format!("{} -A -s v", debian_analyzer::DEFAULT_BUILDER))]
    build_command: String,

    #[arg(long, default_value = "50")]
    max_build_iterations: usize,

    /// Dist command
    #[arg(long, env = "DIST")]
    dist_command: Option<String>,

    /// Debian revision for the new release
    #[arg(long, default_value = "1")]
    debian_revision: String,

    /// Upstream version to package
    #[arg(long)]
    upstream_version: Option<String>,

    /// ognibuild dep server to use
    #[arg(long, env = "OGNIBUILD_DEPS")]
    dep_server_url: Option<String>,

    /// Maintainer team ("$NAME <$EMAIL>")
    #[arg(long)]
    team: Option<String>,

    /// Store output in a temporary directory (just test).
    #[arg(long)]
    discard_output: bool,

    /// Output directory
    #[arg(long)]
    output_directory: Option<PathBuf>,

    /// Attempt to package dependencies if they are not yet packaged.
    #[arg(long, short('r'))]
    recursive: bool,

    /// Name of Debian branch to create. Empty string to stay at current branch.
    #[arg(long, default_value = "%(vendor)s/main")]
    debian_branch: Option<String>,

    /// Package whatever source will create the named Debian binary package.
    #[arg(long)]
    debian_binary: Option<String>,

    /// What kind of release to package
    #[arg(long, default_value = "auto", conflicts_with = "release")]
    upstream_version_kind: VersionKind,

    /// Package latest upstream release rather than a snapshot
    #[arg(long)]
    release: bool,

    /// Upstream to package
    upstream: Option<String>,
}

fn main() -> Result<(), i32> {
    let mut args = Args::parse();

    warn!(
        "debianize is experimental and often generates packaging that is incomplete or does not build as-is. If you encounter issues, please consider filing a bug.");

    if args.release {
        args.upstream_version_kind = VersionKind::Release;
    }

    env_logger::builder()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .filter(
            None,
            if args.debug {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Info
            },
        )
        .init();

    breezyshim::init();

    let compat_release = if let Some(release) = args.compat_release {
        release
    } else {
        debian_analyzer::release_info::resolve_release_codename("stable", None).unwrap()
    };

    let (wt, subpath) = match breezyshim::workingtree::open_containing(&args.directory) {
        Ok((wt, subpath)) => (wt, subpath),
        Err(e) => {
            log::error!(
                "please run debianize in an existing branch where it should add the packaging: {}",
                e
            );
            return Err(1);
        }
    };

    let create_dist_fn = if let Some(dist_command) = args.dist_command {
        fn create_dist_fn(tree: &dyn Tree, package: &str, version: &Version, target_dir: &Path, subpath: &Path) {
            run_dist_command(
                tree,
                package,
                version,
                target_dir,
                dist_command,
                subpath,
            )
        }
        Some(create_dist_fn)
    } else {
        None
    };

    let metadata = upstream_ontologist::UpstreamMetadata::new();

    // For now...
    if let Some(upstream) = args.upstream {
        match breezyshim::branch::open_containing(&upstream.parse().unwrap()) {
            Ok((upstream_branch, upstream_subpath)) => {
                metadata.insert(
                    UpstreamDatumWithMetadata {
                        datum: UpstreamDatum::Repository(upstream),
                        certainty: Some(Certainty::Confident),
                        origin: Some(Origin::User),
                    });
            }
            Err(e) => {
                log::error!("{}: not a valid branch: {}", upstream, e);
                return Err(1);
            }
        }
    } else if let Some(debian_binary) = args.debian_binary {
        let deb_dep = DebianDepenency::from_str(debian_binary).unwrap();
        let upstream_info = find_apt_upstream(deb_dep);
        if upstream_info.is_none() {
            log::error!(
                "{}: Unable to find upstream info for {}",
                debian_binary,
                deb_dep,
            );
            return Err(1);
        }
        log::info!(
            "Found relevant upstream branch at {}", upstream_info.branch_url()
        );
        let upstream_branch = breezyshim::branch::open(upstream_info.branch_url()).unwrap();
        let upstream_subpath = upstream_info.branch_subpath;

        if let Some(url) = upstream_info.branch_url() {
            metadata.insert(
                UpstreamDatumWithMetadata {
                    datum: UpstreamMetadata::Repository(url),
                    certainty: Some(Certainty::Confident),
                    origin: None,
                });
        }
    } else {
        if wt.has_filename(&subpath.join("debian")) {
            report_fatal(
                versions_dict(),
                "debian-directory-exists",
                &format!("{}: A debian directory already exists.", wt.abspath(&subpath)),
                Some("Run lintian-brush instead or specify --force-new-directory."),
                None
            );
            return Err(1);
        }
        log::info!(
            "No upstream repository specified, using upstream source in {}",
            wt.abspath(&subpath).unwrap().display()
        );
        let upstream_branch = wt.branch;
        let upstream_subpath = subpath;
    }

    if let Some(debian_branch) = args.debian_branch {
        use debian_analyzer::vendor::get_vendor_name;

        debianize::use_packaging_branch(
            &wt, &debian_branch.replace("%(vendor)s", get_vendor_name().to_lowercase())
        );
    }

    let use_inotify = if args.disable_inotify {
        Some(false)
    } else {
        None
    };

    let preferences = debianize::DebianizePreferences {
        use_inotify,
        diligence: args.diligence,
        trust: args.trust,
        check: args.check,
        net_access: !args.disable_net_access,
        force_subprocess: args.force_subprocess,
        force_new_directory: args.force_new_directory,
        compat_release: Some(compat_release),
        minimum_certainty: todo!(),
        consult_external_directory: args.consult_external_directory,
        verbose: args.verbose,
        session: if let Some(schroot) = args.schroot {
            log::info!("Using schroot {}", schroot);
            SessionPreferences::Schroot(schroot)
        } else if let Some(unshare) = args.unshare {
            log::info!("Using tarball {} for unshare", unshare);
            SessionPreferences::Unshare(unshare)
        } else {
            SessionPreferences::Plain
        },
        create_dist: create_dist_fn,
        committer: todo!(),
        upstream_version_kind: args.upstream_version_kind,
        debian_revision: args.debian_revision,
        team: todo!(),
        author: todo!(),
    };

    let lock_write = wt.lock_write();

    let debianize_result = debianize::debianize(
        &wt,
        &subpath,
        upstream_branch,
        upstream_subpath,
        &preferences,
        upstream_version,
        &metadata,
        );
        except PackageVersionNotPresent:
            if upstream_version:
                report_fatal(
                    "requested-version-missing",
                    f"Requested version {upstream_version} not present upstream",
                )
                return 1
            else:
                # For now
                raise
        except DistCommandFailed as e:
            report_fatal(e.kind or "dist-command-failed", e.error)  # type: ignore
            return 1
        except WorkspaceDirty:
            report_fatal(
                "pending-changes", "Please commit pending changes first."
            )
            return 1
        except DebianDirectoryExists as e:
            report_fatal(
                code="debian-directory-exists",
                description=f"{e.path}: A debian directory already exists. ",
                hint=(
                    "Run lintian-brush instead or "
                    "specify --force-new-directory."
                ),
            )
            return 1
        except SourcePackageNameInvalid as e:
            report_fatal(
                code="invalid-source-package-name",
                description=(
                    f"Generated source package name {e.source!r} is not valid."
                ),
            )
            return 1
        except NoUpstreamReleases:
            report_fatal(
                "no-upstream-releases",
                "The upstream project does not appear to have "
                "made any releases.",
            )
        except NoBuildToolsFound:
            report_fatal(
                "no-build-tools",
                "Unable to find any build systems in upstream sources.",
            )
            return 1
        except DetailedFailure as e:
            report_fatal(
                "debianize-" + e.error.kind,
                str(e),
                details=(e.error.json() if e.error else None),
            )
            return 1
        except BuildSystemProcessError as e:
            report_fatal("build-system-process-error", e.message)
            return 1
        except OSError as e:
            if e.errno == errno.ENOSPC:
                report_fatal("no-space-on-device", str(e))
                return 1
            else:
                raise

    if install {
        args.iterate_fix = true;
    }

    if args.iterate_fix {
        let session = if let Some(schroot) = args.schroot {
            log::info!("Using schroot {}", schroot);
            ognibuild::session::schroot::SchrootSession::new(schroot).unwrap()
        } else if let Some(unshare) = args.unshare {
            log::info!("Using tarball {} for unshare", unshare);
            ognibuild::session::unshare::UnshareSession::from_tarball(unshare)
        } else {
            PlainSession::new()
        };

        let mut tempdir = None;

        let apt = AptManager::from_session(session);
        let output_directory = if args.discard_output {
            tempdir = Some(tempfile::tempdir().unwrap());
            tempdir.path()
        } else if let Some(output_directory) = args.output_directory {
            output_directory
        } else {
            let output_directory = default_debianize_cache_dir();
            log::info!("Building dependencies in {}", output_directory);
            output_directory
        };

        let do_build = |wt: &WorkingTree, subpath: &Path, incoming_directory: &Path, extra_repositories: Vec<&str| {
            let context = ognibuild::debian::context::DebianPackagingContext {
                tree: wt,
                subpath,
                committer: preferences.committer,
                update_changelog: false,
                commit_reporter: todo!(),
            };
            let fixers = ognibuild::debian::fixers::default_fixers(
                &context,
                apt,
            );
            return ognibuild::debian::fix_build::build_incrementally(
                wt,
                None,
                None,
                incoming_directory,
                build_command,
                fixers,
                None,
                max_build_iterations,
                subpath,
                None,
                None,
                None,
                Some(extra_repositories),
                None
            )
        };

        let (changes_names, _cl_entry) = if recursive {
            let vcs_directory = output_directory.join("vcs");
            std::fs::create_dir_all(&vcs_directory).unwrap();
            let apt_directory = output_directory.join("apt");
            std::fs::create_dir_all(&apt_directory).unwrap();

            let apt_repo = SimpleTrustedAptRepo::new(apt_directory);
            let fixer = debianize::fixers::DebianizeFixer::new(
                            vcs_directory,
                            apt_repo,
                            do_build,
                            dependency,
                            &preferences
                        );
            let (changes_names, _cl_entry) = ognibuild::debian::fix_build::iterate_with_build_fixers(
                    vec![fixer],
                    None,
                    session,
                do_build(
                    &wt,
                    &subpath,
                    apt_repo.directory(),
                    apt_repo.sources_lines(),
                )
                    )
            std::mem::drop(apt_repo);
            (changes_names, _cl_entry)
        } else {
            do_build(
                &wt, &subpath, &output_directory, vec![]
            )
        };
        except DetailedDebianBuildFailure as e:
            if e.phase is None:
                phase = "unknown phase"
            elif len(e.phase) == 1:
                phase = e.phase[0]
            else:
                phase = f"{e.phase[0]} ({e.phase[1]})"
            log::error("Error during {}: {}", phase, e.error);
            return Err(1);
        except UnidentifiedDebianBuildError as e:
            if e.phase is None:
                phase = "unknown phase"
            elif len(e.phase) == 1:
                phase = e.phase[0]
            else:
                phase = f"{e.phase[0]} ({e.phase[1]})"
            logging.fatal("Error during %s: %s", phase, e.description)
            return 1
        except DebianizedPackageRequirementMismatch as e:
            let hint = if upstream_branch {
                Some(format!("Wrong repository ({})?", upstream_branch))
            } else {
                None
            };
            report_fatal(
                versions_dict(),
                "package-requirements-mismatch",
                &format!("Debianized package (binary packages: {:?}), version {} did not satisfy requirement {:?}. ",
                    [binary["Package"] for binary in e.control.binaries],
                    e.version,
                    e.requirement,
                ),
                hint,
            );
        }
        log::info!("Built {:?}.", changes_names);
        if args.install {
            std::process::Command::new("debi")
                .args(changes_names.iter().map(|cn| output_directory.join(cn)))
                .status()
                .unwrap();
        }
    }

    let target_branch_url = if let Some(url) = debianize_result.vcs_url {
        vcs_git_url_to_bzr_url(url)
    } else {
        None
    };

    if svp::enabled() {
        svp::report_success_debian(
            versions_dict(),
            Some(100),
            debianize_result,
        );
    }

    return Ok(());
}

fn versions_dict() -> HashMap<String, String> {
    let mut ret = HashMap::new();
    ret.insert(
        "lintian-brush".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    pyo3::Python::with_gil(|py| {
        let breezy = py.import_bound("breezy").unwrap();
        ret.insert(
            "breezy".to_string(),
            breezy.getattr("version_string").unwrap().extract().unwrap(),
        );

        let debmutate = py.import_bound("debmutate").unwrap();
        ret.insert(
            "debmutate".to_string(),
            debmutate
                .getattr("version_string")
                .unwrap()
                .extract()
                .unwrap(),
        );

        let debian = py.import_bound("debian").unwrap();
        ret.insert(
            "debian".to_string(),
            debian.getattr("__version__").unwrap().extract().unwrap(),
        );

        // TODO(jelmer): Read dependencies from Cargo.lock
    });
    ret
}
