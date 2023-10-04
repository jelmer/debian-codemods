use breezyshim::branch::{open_containing as open_containing_branch, BranchOpenError};
use breezyshim::tree::{MutableTree, WorkingTree, WorkingTreeOpenError};
use clap::Parser;
use debian_changelog::get_maintainer;
use distro_info::DistroInfo;
use log::warn;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;

use debian_analyzer::svp::{
    enabled as svp_enabled, load_resume, report_fatal, report_success_debian,
};
use debian_analyzer::{get_committer, Certainty};
use std::io::Write as _;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamVersionKind {
    Auto,
    Release,
    Snapshot,
}

impl ToString for UpstreamVersionKind {
    fn to_string(&self) -> String {
        match self {
            UpstreamVersionKind::Auto => "auto",
            UpstreamVersionKind::Release => "release",
            UpstreamVersionKind::Snapshot => "snapshot",
        }
        .to_string()
    }
}

impl std::str::FromStr for UpstreamVersionKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(UpstreamVersionKind::Auto),
            "release" => Ok(UpstreamVersionKind::Release),
            "snapshot" => Ok(UpstreamVersionKind::Snapshot),
            _ => Err(format!("Unknown upstream version kind: {}", s)),
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
    diligence: usize,

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
    #[arg(long, default_value = "auto")]
    upstream_version_kind: UpstreamVersionKind,

    /// Package latest upstream release rather than a snapshot
    #[arg(long)]
    release: bool,

    /// Upstream to package
    upstream: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Args::parse();

    warn!(
        "debianize is experimental and often generates packaging that is incomplete or does not build as-is. If you encounter issues, please consider filing a bug.");

    if args.release {
        if args.upstream_version_kind != UpstreamVersionKind::Auto {
            return Err("Cannot specify --release and --upstream-version-kind".into());
        }
        args.upstream_version_kind = UpstreamVersionKind::Release;
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

    let ret: i32 = Python::with_gil(|py| {
        let kwargs = PyDict::new(py);
        kwargs.set_item("directory", args.directory.to_str().unwrap())?;
        kwargs.set_item("disable_inotify", args.disable_inotify)?;
        kwargs.set_item("compat_release", args.compat_release)?;
        kwargs.set_item("verbose", args.verbose)?;
        kwargs.set_item("disable_net_access", args.disable_net_access)?;
        kwargs.set_item("diligence", args.diligence)?;
        kwargs.set_item("trust", args.trust)?;
        kwargs.set_item(
            "consult_external_directory",
            args.consult_external_directory,
        )?;
        kwargs.set_item("check", args.check)?;
        kwargs.set_item("force_subprocess", args.force_subprocess)?;
        kwargs.set_item(
            "force_new_directory",
            args.force_new_directory || args.iterate_fix,
        )?;
        kwargs.set_item("iterate_fix", args.iterate_fix)?;
        kwargs.set_item("install", args.install)?;
        kwargs.set_item("schroot", args.schroot)?;
        kwargs.set_item("unshare", args.unshare)?;
        kwargs.set_item("build_command", args.build_command)?;
        kwargs.set_item("max_build_iterations", args.max_build_iterations)?;
        kwargs.set_item(
            "upstream_version_kind",
            args.upstream_version_kind.to_string(),
        )?;
        kwargs.set_item("recursive", args.recursive)?;
        kwargs.set_item("output_directory", args.output_directory)?;
        kwargs.set_item("discard_output", args.discard_output)?;
        kwargs.set_item("debian_revision", args.debian_revision)?;
        kwargs.set_item("upstream_version", args.upstream_version)?;
        kwargs.set_item("dist_command", args.dist_command)?;
        kwargs.set_item("team", args.team)?;
        kwargs.set_item("debian_branch", args.debian_branch)?;
        kwargs.set_item("debian_binary", args.debian_binary)?;
        kwargs.set_item("dep_server_url", args.dep_server_url)?;
        kwargs.set_item("upstream", args.upstream)?;

        let m = PyModule::import(py, "lintian_brush.debianize")?;
        let debianize = m.getattr("main")?;
        debianize.call((), Some(kwargs))?.extract()
    })
    .unwrap();

    std::process::exit(ret);
}

fn versions_dict() -> HashMap<String, String> {
    let mut ret = HashMap::new();
    ret.insert(
        "lintian-brush".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    pyo3::Python::with_gil(|py| {
        let breezy = py.import("breezy").unwrap();
        ret.insert(
            "breezy".to_string(),
            breezy.getattr("version_string").unwrap().extract().unwrap(),
        );

        let debmutate = py.import("debmutate").unwrap();
        ret.insert(
            "debmutate".to_string(),
            debmutate
                .getattr("version_string")
                .unwrap()
                .extract()
                .unwrap(),
        );

        let debian = py.import("debian").unwrap();
        ret.insert(
            "debian".to_string(),
            debian.getattr("__version__").unwrap().extract().unwrap(),
        );

        // TODO(jelmer): Read dependencies from Cargo.lock
    });
    ret
}
