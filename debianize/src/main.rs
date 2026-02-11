use breezyshim::branch::{Branch, GenericBranch, PyBranch};
use breezyshim::debian::directory::vcs_git_url_to_bzr_url;
use breezyshim::tree::{PyTree, Tree};
use breezyshim::workingtree::{GenericWorkingTree, WorkingTree};
use clap::Parser;
use debversion::Version;
use log::warn;
use std::collections::HashMap;

use std::io::Write as _;
use std::path::{Path, PathBuf};

use breezyshim::debian::VersionKind;
use breezyshim::tree::MutableTree;

use ognibuild::upstream::FindUpstream;

use debianize::simple_apt_repo::SimpleTrustedAptRepo;
use debianize::{Error, SessionPreferences};
use ognibuild::debian::fix_build::IterateBuildError;
use ognibuild::dependencies::debian::DebianDependency;
use upstream_ontologist::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata, UpstreamMetadata};

#[derive(Debug, Clone)]
enum SessionType {
    Plain,
    Schroot(Option<String>),
    Unshare(Option<PathBuf>),
}

impl std::str::FromStr for SessionType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((session_type, value)) = s.split_once(':') {
            match session_type {
                "plain" => Ok(SessionType::Plain),
                "schroot" => Ok(SessionType::Schroot(Some(value.to_string()))),
                "unshare" => Ok(SessionType::Unshare(Some(PathBuf::from(value)))),
                _ => Err(format!("Invalid session type: {}", session_type)),
            }
        } else {
            match s {
                "plain" => Ok(SessionType::Plain),
                "schroot" => Ok(SessionType::Schroot(None)),
                "unshare" => Ok(SessionType::Unshare(None)),
                _ => Err(format!("Invalid session type: {}", s)),
            }
        }
    }
}

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

    /// Session type for isolation: plain, schroot[:name], unshare[:tarball]
    #[arg(long, default_value = "unshare")]
    session: SessionType,

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

    /// Package requirement specification (e.g., ">=1.0.0")
    #[arg(long)]
    requirement: Option<String>,

    /// Force specific build system (override detection)
    #[arg(long)]
    buildsystem: Option<String>,
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

    let dist_command = args.dist_command.clone();
    let create_dist_fn: Option<
        Box<
            dyn for<'a, 'b, 'c, 'd, 'e> Fn(
                &'a dyn PyTree,
                &'b str,
                &'c Version,
                &'d Path,
                &'e Path,
            )
                -> Result<bool, breezyshim::debian::error::Error>,
        >,
    > = if let Some(dist_command) = dist_command {
        Some(Box::new(
            move |tree: &dyn PyTree,
                  package: &str,
                  version: &Version,
                  target_dir: &Path,
                  subpath: &Path|
                  -> Result<bool, breezyshim::debian::error::Error> {
                breezyshim::debian::upstream::run_dist_command(
                    tree,
                    Some(package),
                    version,
                    target_dir,
                    &dist_command,
                    false,
                    subpath,
                )
            },
        ))
    } else {
        None
    };

    let mut svp = svp_client::Reporter::new(versions_dict());

    let mut metadata = UpstreamMetadata::new();

    // For now...
    let (upstream_branch, upstream_subpath) = if let Some(upstream) = args.upstream {
        match breezyshim::branch::open_containing_as_generic(&upstream.parse().unwrap()) {
            Ok((upstream_branch, upstream_subpath)) => {
                metadata.insert(UpstreamDatumWithMetadata {
                    datum: UpstreamDatum::Repository(upstream),
                    certainty: Some(Certainty::Confident),
                    origin: Some(upstream_ontologist::Origin::Other(
                        "command-line".to_string(),
                    )),
                });
                (
                    Box::new(upstream_branch) as Box<dyn Branch>,
                    PathBuf::from(upstream_subpath),
                )
            }
            Err(e) => {
                log::error!("{}: not a valid branch: {}", upstream, e);
                return Err(1);
            }
        }
    } else if let Some(debian_binary) = args.debian_binary {
        let deb_dep = DebianDependency::new(&debian_binary);
        let upstream_info = deb_dep.find_upstream();
        if upstream_info.is_none() {
            log::error!(
                "{}: Unable to find upstream info for {}",
                debian_binary,
                deb_dep.relation_string(),
            );
            return Err(1);
        }
        let upstream_info = upstream_info.unwrap();
        if let Some(url) = upstream_info.repository() {
            log::info!("Found relevant upstream branch at {}", url);
            let (upstream_branch, upstream_subpath) =
                breezyshim::branch::open_containing_as_generic(&url.parse().unwrap()).unwrap();

            metadata.insert(UpstreamDatumWithMetadata {
                datum: UpstreamDatum::Repository(url.to_owned()),
                certainty: Some(Certainty::Confident),
                origin: None,
            });
            (
                Box::new(upstream_branch) as Box<dyn Branch>,
                PathBuf::from(upstream_subpath),
            )
        } else {
            log::error!(
                "{}: Unable to find upstream info for {}",
                debian_binary,
                deb_dep.relation_string(),
            );
            return Err(1);
        }
    } else {
        if wt.has_filename(&subpath.join("debian")) {
            svp.report_fatal(
                "debian-directory-exists",
                &format!(
                    "{}: A debian directory already exists.",
                    wt.abspath(&subpath).unwrap().display()
                ),
                Some("Run lintian-brush instead or specify --force-new-directory."),
                None,
            );
        }
        log::info!(
            "No upstream repository specified, using upstream source in {}",
            wt.abspath(&subpath).unwrap().display()
        );
        (Box::new(wt.branch()) as Box<dyn Branch>, subpath.clone())
    };

    if let Some(debian_branch) = args.debian_branch {
        use debian_analyzer::vendor::get_vendor_name;

        debianize::use_packaging_branch(
            &wt,
            &debian_branch.replace("%(vendor)s", &get_vendor_name().unwrap().to_lowercase()),
        )
        .unwrap();
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
        minimum_certainty: debian_analyzer::Certainty::Confident,
        consult_external_directory: args.consult_external_directory,
        verbose: args.verbose,
        session: match args.session {
            SessionType::Plain => {
                log::info!("Using plain session (no isolation)");
                SessionPreferences::Plain
            }
            SessionType::Schroot(ref name) => {
                let schroot_name = name.clone().or_else(|| std::env::var("SCHROOT").ok());
                if let Some(name) = schroot_name {
                    log::info!("Using schroot: {}", name);
                    SessionPreferences::Schroot(name)
                } else {
                    log::info!("Using default schroot session");
                    SessionPreferences::Schroot("unstable".to_string()) // Default schroot name
                }
            }
            SessionType::Unshare(ref tarball) => {
                if let Some(path) = tarball {
                    log::info!("Using unshare with tarball: {}", path.display());
                    SessionPreferences::Unshare(path.clone())
                } else {
                    log::info!("Using default unshare session");
                    SessionPreferences::Unshare(PathBuf::new()) // Empty path for default
                }
            }
        },
        create_dist: create_dist_fn,
        committer: None,
        upstream_version_kind: args.upstream_version_kind,
        debian_revision: args.debian_revision,
        team: None,
        author: None,
        compat_level: None,
        check_wnpp: true,
        run_fixers: true,
    };

    let lock_write = wt.lock_write();

    let debianize_result = match debianize::debianize(
        &wt,
        &subpath,
        upstream_branch
            .as_any()
            .downcast_ref::<GenericBranch>()
            .map(|gb| gb as &dyn PyBranch),
        Some(&upstream_subpath),
        &preferences,
        args.upstream_version.as_deref(),
        &metadata,
    ) {
        Ok(debianize_result) => debianize_result,
        Err(Error::SubdirectoryNotFound {
            subpath,
            version: _,
        }) => {
            svp.report_fatal(
                "subdirectory-not-found",
                &format!("Subdirectory not found: {}", subpath.display()),
                None,
                None,
            );
        }
        Err(Error::BrzError(e)) => {
            svp.report_fatal(
                "vcs-error",
                &format!("Error running brz: {}", e),
                None,
                None,
            );
        }
        Err(Error::SqlxError(e)) => {
            svp.report_fatal(
                "sql-error",
                &format!("Error running SQL: {}", e),
                None,
                None,
            );
        }
        Err(Error::IoError(e)) => {
            svp.report_fatal(
                "io-error",
                &format!("Error reading files: {}", e),
                None,
                None,
            );
        }
        Err(Error::DebianDirectoryExists(e)) => {
            svp.report_fatal(
                "debian-directory-exists",
                &format!("{}: A debian directory already exists.", e.display()),
                Some("Run lintian-brush instead or specify --force-new-directory."),
                None,
            );
        }
        Err(Error::DebianizedPackageRequirementMismatch {
            dep,
            binary_names,
            version,
            branch,
        }) => {
            svp.report_fatal(
                "debianized-package-requirement-mismatch",
                &format!(
                    "{}: {} requires {} but the debianized package requires {}",
                    dep.relation_string(),
                    binary_names.join(", "),
                    version,
                    branch.map_or("unknown".to_string(), |m| m.to_string())
                ),
                None,
                None,
            );
        }
        Err(Error::EditorError(e)) => {
            svp.report_fatal(
                "editor-error",
                &format!("Error editing files: {}", e),
                None,
                None,
            );
        }
        Err(Error::MissingUpstreamInfo(e)) => {
            svp.report_fatal(
                "missing-upstream-info",
                &format!("Missing upstream info: {}", e),
                None,
                None,
            );
        }
        Err(Error::NoVcsLocation) => {
            svp.report_fatal(
                "no-vcs-location",
                "No VCS location found for the upstream branch.",
                None,
                None,
            );
        }
        Err(Error::NoUpstreamReleases(o)) => {
            svp.report_fatal(
                "no-upstream-releases",
                &if let Some(n) = o {
                    format!("{}: No upstream releases found.", n)
                } else {
                    "No upstream releases found.".to_string()
                },
                None,
                None,
            );
        }
        Err(Error::SourcePackageNameInvalid(name)) => {
            svp.report_fatal(
                "invalid-source-package-name",
                &format!("Generated source package name {} is not valid.", name),
                None,
                None,
            );
        }
        Err(Error::SourceNameUnknown(name)) => {
            svp.report_fatal(
                "source-name-unknown",
                &format!(
                    "Unable to determine source package name{}",
                    name.as_ref()
                        .map(|n| format!(" from {}", n))
                        .unwrap_or_default()
                ),
                None,
                None,
            );
        }
        Err(Error::Other(msg)) => {
            svp.report_fatal("other-error", &msg, None, None);
        }
        Err(Error::ProviderError(e)) => {
            svp.report_fatal(
                "provider-error",
                &format!("Error getting upstream metadata: {:?}", e),
                None,
                None,
            );
        }
        Err(Error::UncommittedChanges) => {
            svp.report_fatal(
                "uncommitted-changes",
                "There are uncommitted changes in the working tree.",
                Some("Commit your changes or use --force to ignore them."),
                None,
            );
        }
    };

    std::mem::drop(lock_write);

    if args.install {
        args.iterate_fix = true;
    }

    if args.iterate_fix {
        #[cfg(target_os = "linux")]
        let session: std::rc::Rc<dyn ognibuild::session::Session> = match &args.session {
            SessionType::Plain => {
                std::rc::Rc::new(ognibuild::session::plain::PlainSession::new()) as _
            }
            SessionType::Schroot(ref name) => {
                let schroot_name = name
                    .clone()
                    .or_else(|| std::env::var("SCHROOT").ok())
                    .unwrap_or_else(|| "unstable".to_string());
                log::info!("Using schroot {}", schroot_name);
                std::rc::Rc::new(
                    ognibuild::session::schroot::SchrootSession::new(&schroot_name, None).unwrap(),
                ) as _
            }
            SessionType::Unshare(ref tarball) => {
                if let Some(path) = tarball {
                    log::info!("Using tarball {} for unshare", path.display());
                    std::rc::Rc::new(
                        ognibuild::session::unshare::UnshareSession::from_tarball(path).unwrap(),
                    ) as _
                } else {
                    log::info!("Using bootstrapped unshare session");
                    std::rc::Rc::new(
                        ognibuild::session::unshare::UnshareSession::bootstrap().unwrap(),
                    ) as _
                }
            }
        };

        #[cfg(not(target_os = "linux"))]
        let session = std::rc::Rc::new(ognibuild::session::plain::PlainSession::new());

        let mut _tempdir = None;

        let output_directory = if args.discard_output {
            _tempdir = Some(tempfile::tempdir().unwrap());
            _tempdir.as_ref().unwrap().path().to_path_buf()
        } else if let Some(output_directory) = args.output_directory {
            output_directory
        } else {
            match debianize::default_debianize_cache_dir() {
                Ok(dir) => {
                    log::info!("Building dependencies in {}", dir.display());
                    dir
                }
                Err(e) => {
                    log::warn!("Failed to access XDG cache directory: {}. Using temporary directory instead.", e);
                    _tempdir = Some(tempfile::tempdir().unwrap());
                    _tempdir.as_ref().unwrap().path().to_path_buf()
                }
            }
        };

        let committer = preferences
            .committer
            .as_ref()
            .map(|c| breezyshim::config::parse_username(c))
            .clone();
        let build_command = args.build_command.clone();

        let do_build = move |wt: &GenericWorkingTree,
                             subpath: &Path,
                             incoming_directory: &Path,
                             extra_repositories: Vec<&str>|
              -> Result<
            ognibuild::debian::build::BuildOnceResult,
            IterateBuildError,
        > {
            let apt = ognibuild::debian::apt::AptManager::from_session(session.as_ref());
            let context = ognibuild::debian::context::DebianPackagingContext::new(
                Clone::clone(wt),
                subpath,
                committer.clone(),
                false,
                Some(Box::new(breezyshim::commit::ReportCommitToLog::new())),
            );
            let fixers = ognibuild::debian::fixers::default_fixers(&context, &apt);
            ognibuild::debian::fix_build::build_incrementally(
                wt,
                None,
                None,
                incoming_directory,
                &build_command.clone(),
                fixers
                    .iter()
                    .map(|f| f.as_ref())
                    .collect::<Vec<_>>()
                    .as_slice(),
                None,
                Some(args.max_build_iterations),
                subpath,
                None,
                None,
                None,
                Some(extra_repositories),
                !context.update_changelog,
            )
        };

        let r = if args.recursive {
            let vcs_directory = output_directory.join("vcs");
            std::fs::create_dir_all(&vcs_directory).unwrap();
            let apt_directory = output_directory.join("apt");
            std::fs::create_dir_all(&apt_directory).unwrap();

            let apt_repo = SimpleTrustedAptRepo::new(apt_directory);
            let debianize_fixer = debianize::fixer::DebianizeFixer::new(
                vcs_directory,
                apt_repo,
                Box::new(do_build),
                &preferences,
            );
            ognibuild::debian::fix_build::build_incrementally(
                &wt,
                None,
                None,
                &output_directory,
                &args.build_command,
                &[&debianize_fixer],
                None,
                Some(args.max_build_iterations),
                &subpath,
                None,
                None,
                None,
                Some(
                    debianize_fixer
                        .apt_repo()
                        .sources_lines()
                        .iter()
                        .map(|l| l.as_str())
                        .collect(),
                ),
                false,
            )
        } else {
            do_build(&wt, &subpath, &output_directory, vec![])
        };
        let buildonceresult = match r {
            Ok(r) => r,
            Err(IterateBuildError::FixerLimitReached(n)) => {
                svp.report_fatal(
                    "fixer-limit-reached",
                    &format!("Reached fixer limit of {} iterations.", n),
                    None,
                    None,
                );
            }
            Err(IterateBuildError::MissingPhase) => {
                svp.report_fatal("missing-phase", "No build phase was specified.", None, None);
            }
            Err(IterateBuildError::Unidentified {
                retcode: _,
                lines,
                secondary,
                phase,
            }) => {
                let hint = if secondary.is_some() {
                    Some("Try running with --verbose.")
                } else {
                    None
                };
                svp.report_fatal(
                    "unidentified-error",
                    &if let Some(phase) = phase {
                        format!("Error during {}: {}", phase, lines.join("\n"))
                    } else {
                        format!("Error: {}", lines.join("\n"))
                    },
                    hint,
                    None,
                );
            }
            Err(IterateBuildError::Persistent(phase, problem)) => {
                svp.report_fatal(
                    "detailed-error",
                    &format!("Error during {}: {}", phase, problem),
                    None,
                    None,
                );
            }
            Err(IterateBuildError::ResetTree(e)) => {
                svp.report_fatal(
                    "reset-tree",
                    &format!("Error resetting tree: {}", e),
                    None,
                    None,
                );
            }
            Err(IterateBuildError::Other(output)) => {
                svp.report_fatal("other-error", &format!("Error: {}", output), None, None);
            }
        };
        log::info!("Built {:?}.", buildonceresult.changes_names);
        if args.install {
            std::process::Command::new("debi")
                .args(
                    buildonceresult
                        .changes_names
                        .iter()
                        .map(|cn| output_directory.join(cn)),
                )
                .status()
                .unwrap();
        }
    }

    let target_branch_url = debianize_result
        .vcs_url
        .as_ref()
        .map(|url| vcs_git_url_to_bzr_url(url.as_ref()));

    if let Some(target_branch_url) = target_branch_url {
        svp.set_target_branch_url(target_branch_url);
    }

    if svp.enabled() {
        svp.report_success_debian(Some(100), Some(debianize_result), None);
    }

    Ok(())
}

fn versions_dict() -> HashMap<String, String> {
    let mut ret = HashMap::new();
    ret.insert(
        "lintian-brush".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    ret
}
