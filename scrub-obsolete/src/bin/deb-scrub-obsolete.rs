use breezyshim::commit::NullCommitReporter;
use breezyshim::error::Error as BrzError;
use breezyshim::tree::MutableTree;
use breezyshim::workingtree::{self, GenericWorkingTree};
use breezyshim::workspace::check_clean_tree;
use breezyshim::WorkingTree;
use clap::Parser;
use debian_analyzer::editor::EditorError;
use debian_analyzer::release_info::resolve_release_codename;
use debian_analyzer::{control_file_present, get_committer, is_debcargo_package};
use debian_workspace::appliers::apply_actions;
use debian_workspace::fs_workspace::FsWorkspace;
use scrub_obsolete::{detect_scrub_obsolete, ScrubObsoleteError, ScrubObsoleteResult};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

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

    let svp = svp_client::Reporter::new(versions_dict());

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
        std::mem::drop(lock_write);
        svp.report_fatal("nothing-to-do", "Package uses debcargo", None, None);
    } else if !control_file_present(&wt, &subpath) {
        std::mem::drop(lock_write);
        svp.report_fatal(
            "missing-control-file",
            "Unable to find debian/control",
            None,
            None,
        );
    }

    let scrub_outcome = scrub_obsolete(
        &wt,
        &subpath,
        &compat_release,
        &upgrade_release,
        update_changelog,
        allow_reformatting,
        args.keep_minimum_depends_versions,
        None,
    );

    std::mem::drop(lock_write);

    let result = match scrub_outcome {
        Ok(r) => r,
        Err(ScrubObsoleteError::EditorError(EditorError::FormattingUnpreservable(p, e))) => {
            for line in e.diff() {
                log::info!("{}", line);
            }
            svp.report_fatal(
                "formatting-unpreservable",
                &format!(
                    "unable to preserve formatting while editing {}",
                    p.display()
                ),
                None,
                None,
            );
        }
        Err(ScrubObsoleteError::EditorError(EditorError::GeneratedFile(p, _e))) => {
            svp.report_fatal(
                "generated-file",
                &format!("unable to edit generated file: {:?}", p),
                None,
                None,
            );
        }
        Err(ScrubObsoleteError::NotDebianPackage(_)) => {
            svp.report_fatal("not-debian-package", "Not a Debian package.", None, None);
        }
        Err(ScrubObsoleteError::EditorError(EditorError::TemplateError(p, _e))) => {
            svp.report_fatal(
                "change-conflict",
                &format!("Generated file changes conflict: {}", p.display()),
                None,
                None,
            );
        }
        Err(ScrubObsoleteError::SqlxError(e)) => {
            svp.report_fatal(
                "udd-error",
                &format!("Error communicating with UDD: {}", e),
                None,
                None,
            );
        }
        Err(ScrubObsoleteError::EditorError(EditorError::BrzError(e))) => {
            svp.report_fatal("brz-error", &format!("Error: {}", e), None, None);
        }
        Err(ScrubObsoleteError::EditorError(EditorError::IoError(e))) => {
            svp.report_fatal("io-error", &format!("Error: {}", e), None, None);
        }
        Err(ScrubObsoleteError::IoError(e)) => {
            svp.report_fatal("io-error", &format!("I/O error: {}", e), None, None);
        }
        Err(ScrubObsoleteError::Workspace(e)) => {
            svp.report_fatal(
                "workspace-error",
                &format!("Workspace error: {}", e),
                None,
                None,
            );
        }
        Err(ScrubObsoleteError::Other(e)) => {
            svp.report_fatal("other-error", &format!("Error: {}", e), None, None);
        }
    };

    if result.any_changes() {
        svp.report_nothing_to_do(Some("no obsolete constraints"), None);
    }

    log::info!("Scrub obsolete settings.");
    for lines in result.itemized().values() {
        for line in lines {
            log::info!("* {}", line);
        }
    }

    svp.report_success_debian(Some(result.value()), Some(result), None);

    Ok(())
}

fn note_changelog_policy(policy: bool, msg: &str) {
    lazy_static::lazy_static! {
        static ref CHANGELOG_POLICY_NOTED: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    }
    if let Ok(mut policy_noted) = CHANGELOG_POLICY_NOTED.lock() {
        if !*policy_noted {
            let extra = if policy {
                "Specify --no-update-changelog to override."
            } else {
                "Specify --update-changelog to override."
            };
            log::info!("{} {}", msg, extra);
        }
        *policy_noted = true;
    }
}

/// Scrub obsolete entries.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::result_large_err)]
fn scrub_obsolete(
    wt: &GenericWorkingTree,
    subpath: &Path,
    compat_release: &str,
    upgrade_release: &str,
    update_changelog: Option<bool>,
    #[allow(unused_variables)] allow_reformatting: bool,
    keep_minimum_depends_versions: bool,
    #[allow(unused_variables)] transitions: Option<HashMap<String, String>>,
) -> Result<ScrubObsoleteResult, ScrubObsoleteError> {
    let debian_path = subpath.join("debian");
    let base_path = wt
        .abspath(subpath)
        .map_err(|e| ScrubObsoleteError::Other(e.to_string()))?;

    // scrub-obsolete doesn't surface package/version metadata to its
    // detectors, so leave them unset rather than fabricating sentinels.
    let ws = FsWorkspace::new(&base_path, None, None);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let detected = rt.block_on(detect_scrub_obsolete(
        &ws,
        compat_release,
        upgrade_release,
        keep_minimum_depends_versions,
    ))?;

    let mut result =
        ScrubObsoleteResult::new(detected.control_actions, detected.maintscript_removed);

    if !result.any_changes() {
        return Ok(result);
    }

    let changed_files = apply_actions(ws.base_path(), &detected.workspace_actions)?;
    // The applier returns paths relative to base_path; promote them to
    // tree-relative paths via the breezy working tree.
    let safe_files: Vec<&Path> = changed_files.iter().map(|p| p.as_path()).collect();
    let mut specific_files: Vec<PathBuf> = wt
        .safe_relpath_files(safe_files.as_slice(), true, false)
        .map_err(|e| ScrubObsoleteError::Other(e.to_string()))?
        .into_iter()
        .collect();

    let summary = result.itemized();

    let changelog_path = debian_path.join("changelog");

    let update_changelog = if let Some(update_changelog) = update_changelog {
        update_changelog
    } else if let Some(dch_guess) =
        debian_analyzer::detect_gbp_dch::guess_update_changelog(wt, &debian_path, None)
    {
        note_changelog_policy(dch_guess.update_changelog, &dch_guess.explanation);
        dch_guess.update_changelog
    } else {
        // If we can't guess, default to updating the changelog.
        true
    };

    if update_changelog {
        let mut lines = vec![];
        for (release, entries) in summary.iter() {
            let rev_aliases = debian_analyzer::release_info::release_aliases(release, None);
            let mut line = format!("Remove constraints unnecessary since {}", release);
            for alias in rev_aliases {
                line += &format!(" ({})", alias);
            }
            line += ":";
            lines.push(line);
            lines.extend(entries.iter().map(|x| format!("* {}", x)));
        }
        debian_analyzer::add_changelog_entry(
            wt,
            &changelog_path,
            lines
                .iter()
                .map(|x| x.as_str())
                .collect::<Vec<_>>()
                .as_slice(),
        )?;
        specific_files.push(changelog_path);
    }

    result.set_specific_files(specific_files.clone());

    let mut lines = vec![];
    for (release, _entries) in summary.iter() {
        let rev_aliases = debian_analyzer::release_info::release_aliases(release, None);
        let mut line = format!("Remove constraints unnecessary since {}", release);
        for alias in rev_aliases {
            line += &format!(" ({})", alias);
        }
        line += ":";

        lines.push(line);
    }
    lines.extend(["".to_string(), "Changes-By: deb-scrub-obsolete".to_string()]);

    let committer = debian_analyzer::get_committer(wt);

    match wt
        .build_commit()
        .specific_files(
            specific_files
                .iter()
                .map(|x| x.as_path())
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .message(&lines.join("\n"))
        .allow_pointless(false)
        .reporter(&NullCommitReporter::new())
        .committer(&committer)
        .commit()
    {
        Ok(_) | Err(BrzError::PointlessCommit) => {}
        Err(e) => {
            return Err(ScrubObsoleteError::Other(e.to_string()));
        }
    }

    Ok(result)
}
