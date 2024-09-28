use breezyshim::error::Error as BrzError;
use breezyshim::tree::MutableTree;
use breezyshim::workingtree;
use breezyshim::workspace::check_clean_tree;
use clap::Parser;
use debian_analyzer::editor::EditorError;
use debian_analyzer::release_info::resolve_release_codename;
use debian_analyzer::svp::{report_fatal, report_nothing_to_do, report_success_debian};
use debian_analyzer::{control_file_present, get_committer, is_debcargo_package};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version)]
struct Args {
    /// directory to run in
    #[clap(short, long, default_value = ".")]
    directory: PathBuf,

    /// Release to allow upgrading from
    #[clap(short, long, default_value = "oldstable")]
    upgrade_release: String,

    /// Release to allow building on
    #[clap(short, long, env = "COMPAT_RELEASE")]
    compat_release: Option<String>,

    /// do not update the changelog
    #[clap(long)]
    no_update_changelog: bool,

    /// update the changelog
    #[clap(long)]
    update_changelog: Option<bool>,

    #[clap(long, hide = true)]
    allow_reformatting: Option<bool>,

    #[clap(long)]
    /// Keep minimum version dependencies, even when unnecessary
    keep_minimum_depends_versions: bool,

    #[clap(long)]
    /// Print user identity that would be used when committing
    identity: bool,

    #[clap(long)]
    /// Describe all considered changes
    debug: bool,
}

fn versions_dict() -> HashMap<String, String> {
    let mut versions = HashMap::new();
    versions.insert(
        "deb-scrub-obsolete".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    versions.insert(
        "breezy".to_string(),
        breezyshim::version::version().to_string(),
    );
    versions
}

fn main() -> Result<(), i32> {
    let args = Args::parse();

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

    let (wt, subpath) = match workingtree::open_containing(&args.directory) {
        Ok((wt, sp)) => (wt, sp),
        Err(BrzError::NotBranchError(..)) => {
            log::error!("No version control directory found (e.g. a .git directory).");
            return Err(1);
        }
        Err(e) => {
            log::error!("Unable to open local tree: {}", e);
            return Err(1);
        }
    };

    if args.identity {
        log::info!("{}", get_committer(&wt));
        return Ok(());
    }

    let lock_write = wt.lock_write();
    match check_clean_tree(&wt, &wt.basis_tree().unwrap(), &subpath) {
        Ok(()) => {}
        Err(BrzError::WorkspaceDirty(..)) => {
            log::info!(
                "{}: Please commit pending changes first.",
                wt.basedir().display()
            );
            return Err(1);
        }
        Err(e) => {
            log::error!("Unable to check for pending changes: {}", e);
            return Err(1);
        }
    }

    let mut update_changelog = args.update_changelog;
    let mut allow_reformatting = args.allow_reformatting;
    let upgrade_release = resolve_release_codename(&args.upgrade_release, None).unwrap();
    let mut compat_release = args
        .compat_release
        .map(|r| resolve_release_codename(&r, None).unwrap());

    match debian_analyzer::config::Config::from_workingtree(&wt, &subpath) {
        Ok(cfg) => {
            update_changelog = update_changelog.or(cfg.update_changelog());
            allow_reformatting = allow_reformatting.or(cfg.allow_reformatting());
            compat_release = compat_release.or(cfg.compat_release());
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            log::error!("Unable to read configuration: {}", e);
            return Err(1);
        }
    };

    let compat_release =
        compat_release.unwrap_or_else(|| resolve_release_codename("oldstable", None).unwrap());

    if upgrade_release != compat_release {
        log::info!(
            "Removing run time constraints unnecessary since {} and build time constraints unnecessary since {}",
            upgrade_release,
            compat_release,
        );
    } else {
        log::info!(
            "Removing run time and build time constraints unnecessary since {}",
            compat_release,
        );
    }

    let allow_reformatting = allow_reformatting.unwrap_or(false);

    if is_debcargo_package(&wt, &subpath) {
        report_fatal(
            versions_dict(),
            "nothing-to-do",
            "Package uses debcargo",
            None,
            None,
        );
    } else if !control_file_present(&wt, &subpath) {
        report_fatal(
            versions_dict(),
            "missing-control-file",
            "Unable to find debian/control",
            None,
            None,
        );
    }

    let result = match scrub_obsolete::scrub_obsolete(
        wt,
        &subpath,
        &compat_release,
        &upgrade_release,
        update_changelog,
        allow_reformatting,
        args.keep_minimum_depends_versions,
        None,
    ) {
        Ok(r) => r,
        Err(scrub_obsolete::ScrubObsoleteError::EditorError(
            EditorError::FormattingUnpreservable(p, e),
        )) => {
            for line in e.diff() {
                log::info!("{}", line);
            }
            report_fatal(
                versions_dict(),
                "formatting-unpreservable",
                &format!(
                    "unable to preserve formatting while editing {}",
                    p.display()
                ),
                None,
                None,
            );
        }
        Err(scrub_obsolete::ScrubObsoleteError::EditorError(EditorError::GeneratedFile(p, _e))) => {
            report_fatal(
                versions_dict(),
                "generated-file",
                &format!("unable to edit generated file: {:?}", p),
                None,
                None,
            );
        }
        Err(scrub_obsolete::ScrubObsoleteError::NotDebianPackage(_)) => {
            report_fatal(
                versions_dict(),
                "not-debian-package",
                "Not a Debian package.",
                None,
                None,
            );
        }
        Err(scrub_obsolete::ScrubObsoleteError::EditorError(EditorError::TemplateError(p, _e))) => {
            report_fatal(
                versions_dict(),
                "change-conflict",
                &format!("Generated file changes conflict: {}", p.display()),
                None,
                None,
            );
        }
        Err(scrub_obsolete::ScrubObsoleteError::SqlxError(e)) => {
            report_fatal(
                versions_dict(),
                "udd-error",
                &format!("Error communicating with UDD: {}", e),
                None,
                None,
            );
        }
        Err(
            scrub_obsolete::ScrubObsoleteError::BrzError(e)
            | scrub_obsolete::ScrubObsoleteError::EditorError(EditorError::BrzError(e)),
        ) => {
            report_fatal(
                versions_dict(),
                "brz-error",
                &format!("Error: {}", e),
                None,
                None,
            );
        }
        Err(scrub_obsolete::ScrubObsoleteError::EditorError(EditorError::IoError(e))) => {
            report_fatal(
                versions_dict(),
                "io-error",
                &format!("Error: {}", e),
                None,
                None,
            );
        }
    };

    std::mem::drop(lock_write);

    if result.any_changes() {
        report_nothing_to_do(versions_dict(), Some("no obsolete constraints"));
    }

    log::info!("Scrub obsolete settings.");
    for lines in result.itemized().values() {
        for line in lines {
            log::info!("* {}", line);
        }
    }

    report_success_debian(versions_dict(), Some(result.value()), Some(result), None);

    Ok(())
}
