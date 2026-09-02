use breezyshim::branch::open_containing_as_generic as open_containing_branch;
use breezyshim::dirty_tracker::DirtyTreeTracker;
use breezyshim::error::Error;
use breezyshim::repository::Repository;
use breezyshim::tree::MutableTree;
use breezyshim::workingtree::GenericWorkingTree;
use breezyshim::workspace::check_clean_tree;
use breezyshim::{Branch, WorkingTree};
use clap::Parser;
use debian_analyzer::detect_gbp_dch::{guess_update_changelog, ChangelogBehaviour};
use debian_analyzer::{
    add_changelog_entry, apply_or_revert, control_file_present, get_committer, is_debcargo_package,
    ApplyError, Certainty, ChangelogError,
};
use debian_changelog::get_maintainer;
use debian_workspace::appliers::apply_actions;
use multiarch_hints::{
    cache_download_multiarch_hints, detect_multiarch_hints, multiarch_hints_by_binary,
    parse_multiarch_hints, ApplyMultiarchHintsConfig, Change, Hint, OverallResult,
};
use std::collections::HashMap;
use std::io::Write as _;
use svp_client::Reporter;

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    #[arg(long, hide = true)]
    minimum_certainty: Option<Certainty>,

    /// Allow file reformatting and stripping of comments
    #[arg(short, long)]
    allow_reformatting: Option<bool>,

    /// Be verbose
    #[arg(short, long, default_value_t = std::env::var("SVP_API").is_ok())]
    verbose: bool,

    /// Print resulting diff afterwards
    #[arg(long, default_value_t = false)]
    diff: bool,

    /// Enable debug output
    #[arg(long, default_value_t = false)]
    debug: bool,

    /// Do not make any changes to the current repository.
    /// Note: currently creates a temporary clone of the repository.
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Print user identity that would be used when committing
    #[arg(long, default_value_t = false)]
    identity: bool,

    /// directory to run in
    #[arg(short, long, default_value = std::env::current_dir().unwrap().into_os_string(), value_name = "DIR")]
    directory: std::path::PathBuf,

    /// Do not probe external services
    #[arg(long, default_value_t = false)]
    disable_net_access: bool,

    /// Disable inotify
    #[arg(long, default_value_t = false, hide = true)]
    disable_inotify: bool,

    /// Document changes in the changelog [default: auto-detect]
    #[arg(long, default_value_t = false, conflicts_with = "no_update_changelog")]
    update_changelog: bool,

    /// Do not document changes in the changelog (useful when using e.g. "gbp dch") [default: auto-detect]
    #[arg(long, default_value_t = false, conflicts_with = "update_changelog")]
    no_update_changelog: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct AppliedHint {
    action: String,
    certainty: Certainty,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct MultiArchResult {
    #[serde(rename = "applied-hints")]
    applied_hints: Vec<AppliedHint>,
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

fn no_committer_hint(tree: &GenericWorkingTree) -> String {
    match tree.branch().vcs_type() {
        breezyshim::foreign::VcsType::Git => "No committer identity configured; run \
            `git config --global user.name \"Your Name\"` and \
            `git config --global user.email you@example.com`."
            .to_string(),
        _ => "No committer identity configured; \
            run `brz whoami \"Your Name <you@example.com>\"`."
            .to_string(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    let mut update_changelog: Option<bool> = if args.update_changelog {
        Some(true)
    } else if args.no_update_changelog {
        Some(false)
    } else {
        None
    };

    let mut tempdir = None;

    let (wt, subpath) = if args.dry_run {
        let (branch, subpath) = match open_containing_branch(
            &url::Url::from_directory_path(&args.directory).unwrap(),
        ) {
            Ok((branch, subpath)) => (branch, subpath),
            Err(Error::NotBranchError(_msg, _)) => {
                log::error!("No version control directory found (e.g. a .git directory).");
                std::process::exit(1);
            }
            Err(Error::DependencyNotPresent(name, _reason)) => {
                log::error!(
                    "Unable to open branch at {}: missing package {}",
                    args.directory.display(),
                    name
                );
                std::process::exit(1);
            }
            Err(Error::NoColocatedBranchSupport) => {
                panic!("NoColocatedBranchSupport should not be returned by open_containing");
            }
            Err(err) => {
                log::error!(
                    "Unable to open branch at {}: {}",
                    args.directory.display(),
                    err
                );
                std::process::exit(1);
            }
        };

        let td = tempfile::tempdir()?;

        // TODO(jelmer): Make a slimmer copy

        let to_dir = branch.controldir().sprout(
            url::Url::from_directory_path(td.path()).unwrap(),
            None,
            Some(true),
            Some(branch.format().supports_stacking()),
            None,
        )?;
        tempdir = Some(td);
        (
            to_dir.open_workingtree()?,
            std::path::PathBuf::from(subpath),
        )
    } else {
        match breezyshim::workingtree::open_containing(&args.directory) {
            Ok((wt, subpath)) => (wt, subpath),
            Err(Error::NotBranchError(_msg, _)) => {
                log::error!("No version control directory found (e.g. a .git directory).");
                std::process::exit(1);
            }
            Err(Error::DependencyNotPresent(name, _reason)) => {
                log::error!(
                    "Unable to open tree at {}: missing package {}",
                    args.directory.display(),
                    name
                );
                std::process::exit(1);
            }
            Err(e) => {
                log::error!("Unable to open tree at {}: {}", args.directory.display(), e);
                std::process::exit(1);
            }
        }
    };
    if args.identity {
        println!("Committer identity: {}", get_committer(&wt));
        let (maintainer, email) = get_maintainer().unwrap_or(("".into(), "".into()));
        println!("Changelog identity: {} <{}>", maintainer, email);
        std::process::exit(0);
    }

    match check_clean_tree(&wt, &wt.basis_tree().unwrap(), subpath.as_path()) {
        Err(Error::WorkspaceDirty(p)) => {
            log::error!(
                "{}: Please commit pending changes and remove unknown files first.",
                p.display()
            );
            if args.verbose {
                breezyshim::status::show_tree_status(&wt).unwrap();
            }
            std::process::exit(1);
        }
        Err(e) => {
            log::error!("Internal error: {}", e);
            std::process::exit(1);
        }
        Ok(_) => {}
    };

    let since_revid = wt.last_revision().unwrap();
    let mut minimum_certainty = args.minimum_certainty;
    let mut allow_reformatting = args.allow_reformatting;
    match debian_analyzer::config::Config::from_workingtree(&wt, subpath.as_path()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            log::error!("Unable to read config: {}", e);
            std::process::exit(1);
        }
        Ok(cfg) => {
            if minimum_certainty.is_none() {
                minimum_certainty = cfg.minimum_certainty();
            }
            if allow_reformatting.is_none() {
                allow_reformatting = cfg.allow_reformatting();
            }
            if update_changelog.is_none() {
                update_changelog = cfg.update_changelog();
            }
        }
    }

    let mut changelog_behaviour = None;
    let update_changelog = update_changelog.unwrap_or_else(|| {
        let debian_path = subpath.join("debian");
        changelog_behaviour = guess_update_changelog(&wt, debian_path.as_path(), None);
        if let Some(behaviour) = changelog_behaviour.as_ref() {
            note_changelog_policy(behaviour.update_changelog, behaviour.explanation.as_str());
            behaviour.update_changelog
        } else {
            // If we can't make an educated guess, assume yes.
            changelog_behaviour = Some(ChangelogBehaviour {
                update_changelog: true,
                explanation: "Assuming changelog should be updated".to_string(),
            });
            true
        }
    });

    let svp = Reporter::new(versions_dict());

    let write_lock = wt.lock_write();

    let text = match cache_download_multiarch_hints(None) {
        Ok(text) => text,
        Err(e) => {
            drop(write_lock);
            svp.report_fatal(
                "multiarch-hints-download-error",
                format!("Unable to download multiarch hints: {:?}", e).as_str(),
                None,
                Some(true),
            );
        }
    };

    let hints = parse_multiarch_hints(text.as_slice()).unwrap();
    let hints = multiarch_hints_by_binary(hints.as_slice());

    if debian_analyzer::control_files_in_root(&wt, subpath.as_path()) {
        drop(write_lock);
        svp.report_fatal(
            "control-files-in-root",
            "control files live in root rather than debian/ (LarstIQ mode)",
            None,
            None,
        );
    }

    if is_debcargo_package(&wt, subpath.as_path()) {
        drop(write_lock);
        svp.report_nothing_to_do(Some("Package uses debcargo"), None);
    }

    if !control_file_present(&wt, subpath.as_path()) {
        drop(write_lock);
        svp.report_fatal(
            "missing-control-file",
            "Unable to find debian/control",
            None,
            None,
        );
    }

    let mut dirty_tracker = if !args.disable_inotify {
        Some(DirtyTreeTracker::new_in_subpath(
            Clone::clone(&wt),
            subpath.as_path(),
        ))
    } else {
        None
    };

    let config = ApplyMultiarchHintsConfig {
        minimum_certainty,
        committer: None,
        update_changelog,
        allow_reformatting,
    };

    let result = match apply_multiarch_hints(
        &wt,
        subpath.as_path(),
        &hints,
        dirty_tracker.as_mut(),
        &config,
    ) {
        Err(OverallError::NoChanges) => {
            drop(write_lock);
            svp.report_nothing_to_do(None, None);
        }
        Err(OverallError::NotDebianPackage(p)) => {
            drop(write_lock);
            svp.report_fatal(
                "not-debian-package",
                format!("{}: Not a Debian package", p.display()).as_str(),
                None,
                None,
            );
        }
        Err(OverallError::BrzError(e)) => {
            drop(write_lock);
            svp.report_fatal(
                "internal-error",
                format!("Tree manipulation error: {}", e).as_str(),
                None,
                None,
            );
        }
        Err(OverallError::Other(e)) => {
            drop(write_lock);
            svp.report_fatal(
                "internal-error",
                format!("Error: {}", e).as_str(),
                None,
                None,
            );
        }
        Err(OverallError::NoWhoami) => {
            drop(write_lock);
            svp.report_fatal("no-whoami", &no_committer_hint(&wt), None, None);
        }
        Err(OverallError::GeneratedFile(p)) => {
            drop(write_lock);
            svp.report_fatal(
                "generated-file",
                format!("{}: File is generated", p.display()).as_str(),
                None,
                None,
            );
        }
        Err(OverallError::FormattingUnpreservable(p)) => {
            drop(write_lock);
            svp.report_fatal(
                "unpreservable-formatting",
                format!("{}: Unable to preserve formatting", p.display()).as_str(),
                None,
                None,
            );
        }
        Ok(overall_result) => overall_result,
    };
    std::mem::drop(write_lock);
    if let Some(tempdir) = tempdir {
        if let Err(e) = tempdir.close() {
            log::warn!("Error removing temporary directory: {}", e);
        }
    }

    let mut applied_hints = result
        .changes
        .iter()
        .map(|x| AppliedHint {
            action: x.hint.kind().to_string(),
            certainty: x.certainty,
        })
        .collect::<Vec<_>>();

    for change in &result.changes {
        log::info!("{}: {}", change.binary, change.description);
    }

    if args.diff {
        breezyshim::diff::show_diff_trees(
            &wt.branch()
                .repository()
                .revision_tree(&since_revid)
                .unwrap(),
            &wt,
            Box::new(std::io::stdout()),
            None,
            None,
        )?;
    }
    if svp.enabled() {
        if let Some(base) = svp.load_resume::<Vec<AppliedHint>>() {
            applied_hints.extend(base);
        }
        svp.report_success_debian(
            Some(result.value()),
            Some(MultiArchResult { applied_hints }),
            changelog_behaviour.map(|x| x.into()),
        )
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

#[derive(Debug)]
enum OverallError {
    BrzError(Error),
    NotDebianPackage(std::path::PathBuf),
    Other(String),
    NoWhoami,
    NoChanges,
    GeneratedFile(std::path::PathBuf),
    FormattingUnpreservable(std::path::PathBuf),
}

impl From<debian_analyzer::editor::EditorError> for OverallError {
    fn from(e: debian_analyzer::editor::EditorError) -> Self {
        match e {
            debian_analyzer::editor::EditorError::GeneratedFile(p, _) => {
                OverallError::GeneratedFile(p)
            }
            debian_analyzer::editor::EditorError::FormattingUnpreservable(p, _) => {
                OverallError::FormattingUnpreservable(p)
            }
            debian_analyzer::editor::EditorError::BrzError(e) => OverallError::BrzError(e),
            debian_analyzer::editor::EditorError::IoError(e) => OverallError::Other(e.to_string()),
            debian_analyzer::editor::EditorError::TemplateError(p, _e) => {
                OverallError::GeneratedFile(p)
            }
        }
    }
}

impl std::fmt::Display for OverallError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            OverallError::NotDebianPackage(p) => {
                write!(f, "{} is not a Debian package.", p.display())
            }
            OverallError::GeneratedFile(p) => {
                write!(f, "Generated file: {}", p.display())
            }
            OverallError::FormattingUnpreservable(p) => {
                write!(f, "Formatting unpreservable: {}", p.display())
            }
            OverallError::BrzError(e) => write!(f, "{}", e),
            OverallError::NoWhoami => write!(f, "No committer configured."),
            OverallError::NoChanges => write!(f, "No changes to apply."),
            OverallError::Other(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for OverallError {}

impl From<Error> for OverallError {
    fn from(e: Error) -> Self {
        match e {
            Error::PointlessCommit => OverallError::NoChanges,
            Error::NoWhoami => OverallError::NoWhoami,
            Error::Other(e) => OverallError::Other(e.to_string()),
            e => OverallError::BrzError(e),
        }
    }
}

impl From<ChangelogError> for OverallError {
    fn from(e: ChangelogError) -> Self {
        match e {
            ChangelogError::NotDebianPackage(p) => OverallError::NotDebianPackage(p),
            ChangelogError::Python(e) => OverallError::Other(e.to_string()),
        }
    }
}

fn changes_by_description(changes: &[Change]) -> HashMap<String, Vec<String>> {
    let mut by_description: HashMap<String, Vec<String>> = HashMap::new();
    for change in changes {
        by_description
            .entry(change.description.clone())
            .or_default()
            .push(change.binary.clone());
    }
    by_description
}

#[allow(clippy::result_large_err)]
fn apply_multiarch_hints(
    local_tree: &GenericWorkingTree,
    subpath: &std::path::Path,
    hints: &HashMap<&str, Vec<&Hint>>,
    dirty_tracker: Option<&mut DirtyTreeTracker>,
    config: &ApplyMultiarchHintsConfig,
) -> Result<OverallResult, OverallError> {
    let minimum_certainty = config.minimum_certainty.unwrap_or(Certainty::Certain);
    let basis_tree = local_tree.basis_tree().map_err(OverallError::BrzError)?;
    let (changes, _tree_changes, mut specific_files) = match apply_or_revert(
        local_tree,
        subpath,
        &basis_tree,
        dirty_tracker,
        |path| -> Result<Vec<Change>, OverallError> {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(path, None, None);
            let detected = detect_multiarch_hints(&ws, hints, minimum_certainty)
                .map_err(|e| OverallError::Other(e.to_string()))?;

            if detected.is_empty() {
                return Ok(Vec::new());
            }

            let all_actions: Vec<_> = detected
                .iter()
                .flat_map(|(_, plan)| plan.actions.iter().cloned())
                .collect();
            apply_actions(path, &all_actions).map_err(|e| OverallError::Other(e.to_string()))?;

            Ok(detected.into_iter().map(|(change, _)| change).collect())
        },
    ) {
        Ok(r) => r,
        Err(ApplyError::NoChanges(_)) => return Err(OverallError::NoChanges),
        Err(ApplyError::BrzError(e)) => return Err(OverallError::BrzError(e)),
        Err(ApplyError::CallbackError(_)) => panic!("Unexpected callback error"),
    };

    let by_description = changes_by_description(changes.as_slice());
    let mut overall_description = vec!["Apply multi-arch hints.\n".to_string()];
    for (description, mut binaries) in by_description {
        binaries.sort();
        overall_description.push(format!(" + {}: {}\n", binaries.join(", "), description));
    }

    let changelog_path = subpath.join("debian/changelog");

    if config.update_changelog {
        add_changelog_entry(
            local_tree,
            changelog_path.as_path(),
            overall_description
                .iter()
                .map(|x| x.as_str())
                .collect::<Vec<_>>()
                .as_slice(),
        )?;
        if let Some(specific_files) = specific_files.as_mut() {
            specific_files.push(changelog_path);
        }
    }

    overall_description.push("\n".to_string());
    overall_description.push("Changes-By: apply-multiarch-hints\n".to_string());

    let committer = config
        .committer
        .clone()
        .unwrap_or_else(|| get_committer(local_tree));

    let specific_files_ref = specific_files
        .as_ref()
        .map(|x| x.iter().map(|x| x.as_path()).collect::<Vec<_>>());

    let mut builder = local_tree
        .build_commit()
        .message(overall_description.concat().as_str())
        .allow_pointless(false)
        .committer(&committer);

    if let Some(specific_files) = specific_files_ref.as_deref() {
        builder = builder.specific_files(specific_files);
    }

    builder.commit()?;

    Ok(OverallResult { changes })
}
