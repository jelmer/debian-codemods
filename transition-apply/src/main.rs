use clap::Parser;
use std::path::{Path,PathBuf};
use std::io::Write;
use std::collections::HashMap;
use breezyshim::error::Error as BrzError;
use deb_transition_apply::TransitionResult;
use debian_analyzer::config::Config;
use debian_analyzer::transition::Transition;
use debian_analyzer::svp::{enabled, report_fatal, report_success_debian, report_nothing_to_do};
use debian_analyzer::editor::EditorError;
use debian_analyzer::control::TemplatedControlEditor;
use breezyshim::workingtree::{self, WorkingTree};

#[derive(Parser)]
struct Args {
    #[clap(long, default_value = ".")]
    /// directory to run in
    directory: PathBuf,

    #[clap(long)]
    /// do not update the changelog
    no_update_changelog: bool,

    #[clap(long)]
    /// force updating of the changelog
    update_changelog: bool,

    #[clap(long, hide = true)]
    /// allow reformatting
    allow_reformatting: bool,

    #[clap(long)]
    /// Print user identity that would be used when committing
    identity: bool,

    #[clap(long)]
    /// Describe all considered changes.
    debug: bool,

    /// Benfile to read transition from.
    benfile: PathBuf,
}

fn apply_transition(wt: &WorkingTree, debian_path: &Path, transition: &Transition) -> Result<TransitionResult, EditorError> {
    use debian_analyzer::control::TemplatedControlEditor;

    let control_path = debian_path.join("control");

    let mut editor = TemplatedControlEditor::create(wt.abspath(&control_path).unwrap())?;

    Ok(deb_transition_apply::apply_transition(&mut editor, transition))
}

fn versions_dict() -> HashMap<String, String> {
    let mut versions = HashMap::new();
    versions.insert("deb-transition-apply".to_string(), env!("CARGO_PKG_VERSION").to_string());
    versions
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

    let mut f = match std::fs::File::open(&args.benfile) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Unable to open benfile: {}", e);
            std::process::exit(1);
        }
    };

    let transition = match debian_analyzer::transition::read_transition(&mut f) {
        Ok(transition) => {
            transition
        }
        Err(e) => {
            log::error!("Unable to read benfile: {}", e);
            std::process::exit(1);
        }
    };

    let (wt, subpath) = match breezyshim::workingtree::open_containing(&args.directory) {
        Ok((wt, sp)) => (wt, sp),
        Err(e) => {
            log::error!("No working tree found in {}: {}", args.directory.display(), e);
            std::process::exit(1);
        }
    };

    if args.identity {
        log::info!("{}", debian_analyzer::get_committer(&wt));
        return Ok(());
    }

    match breezyshim::workspace::check_clean_tree(&wt, &wt.basis_tree().unwrap(), &subpath) {
        Ok(_) => {}
        Err(BrzError::WorkspaceDirty(..)) => {
            log::info!("{}: Please commit pending changes first.", wt.basedir().display());
            return Ok(());
        }
        Err(e) => {
            log::error!("Unable to check tree cleanliness: {}", e);
            std::process::exit(1);
        }
    };

    let mut update_changelog = if args.update_changelog {
        Some(true)
    } else if args.no_update_changelog {
        Some(false)
    } else {
        None
    };
    let mut allow_reformatting = if args.allow_reformatting {
        Some(true)
    } else {
        None
    };

    match Config::from_workingtree(&wt, &subpath) {
        Ok(cfg) => {
            if update_changelog.is_none() {
                update_changelog = cfg.update_changelog();
            }
            if allow_reformatting.is_none() {
                allow_reformatting = cfg.allow_reformatting();
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(e) => {
            log::error!("Unable to read config: {}", e);
            std::process::exit(1);
        }
    }

    let allow_reformatting = allow_reformatting.unwrap_or(false);

    let debian_path = if debian_analyzer::control_files_in_root(&wt, &subpath) {
        subpath.to_path_buf()
    } else {
        subpath.join("debian")
    };

    let (result, bugnos) = match crate::apply_transition(
            &wt,
            &debian_path,
            &transition,
        ) {
        Ok(crate::TransitionResult::PackageNotAffected(..)) => {
            report_nothing_to_do(versions_dict(), Some("Package not affected by transition"));
        }
        Ok(crate::TransitionResult::PackageAlreadyGood(..)) => {
            report_nothing_to_do(versions_dict(), Some("Package is already in a good state"));
        }
        Ok(crate::TransitionResult::PackageNotBad(..)) => {
            report_nothing_to_do(versions_dict(), Some("Package is not in a bad state"));
        }
        Ok(TransitionResult::TransitionSuccess(result, bugnos)) => (result, bugnos),
        Ok(TransitionResult::Unsupported(..)) => {
            report_fatal(versions_dict(), "unsupported-transition", "Unsupported transition", None, Some(false));
        }
        Err(e) => {
            log::error!("Unable to apply transition: {}", e);
            std::process::exit(1);
        }
    };

    let changelog_path = debian_path.join("changelog");

    let (update_changelog, changelog_explanation) = if let Some(update_changelog) = update_changelog {
        (update_changelog, "Specified by --update-changelog or --no-update-changelog".to_string())
    } else {
        if let Some(dch_guess) = debian_analyzer::detect_gbp_dch::guess_update_changelog(&wt, &debian_path, None) {
            note_changelog_policy(dch_guess.update_changelog, &dch_guess.explanation);
            (dch_guess.update_changelog, dch_guess.explanation)
        } else {
            (true, "No changelog policy detected".to_string())
        }
    };

    if update_changelog {
        let mut summary = format!("Apply transition {}. ", transition.title.unwrap());
        if !bugnos.is_empty() {
            summary.push_str(&format!("Closes: {}", bugnos.iter().map(|b| format!("#{}", b)).collect::<Vec<_>>().join(", ")));
        }
        match debian_analyzer::add_changelog_entry(&wt, &changelog_path, &[&summary]) {
            Ok(_) => {},
            Err(e) => {
                log::error!("Unable to update changelog: {}", e);
                std::process::exit(1);
            }
        }
    }

    report_success_debian(versions_dict(), Some(10), Some(result), Some((update_changelog, changelog_explanation)));
    Ok(())
}
