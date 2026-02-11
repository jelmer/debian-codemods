use breezyshim::branch::open_containing_as_generic as open_containing_branch;
use breezyshim::error::Error;
use breezyshim::repository::Repository;
use breezyshim::tree::MutableTree;
use breezyshim::workingtree;
use breezyshim::{Branch, WorkingTree};
use clap::Parser;
use debian_changelog::get_maintainer;
use distro_info::DistroInfo;

use debian_analyzer::{get_committer, Certainty};
use lintian_brush::{ManyResult, OverallError};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(clap::Args, Clone, Debug)]
#[group()]
struct FixerArgs {
    /// Specific fixers to run
    fixers: Option<Vec<String>>,

    /// Path to fixer scripts
    #[arg(short, long)]
    fixers_dir: Option<PathBuf>,

    /// Exclude fixers
    #[arg(long, value_name = "EXCLUDE", help_heading = Some("Fixers"))]
    exclude: Option<Vec<String>>,

    /// Use features/compatibility levels that are not available in stable. (makes backporting
    /// harder)
    #[arg(long, conflicts_with = "compat_release")]
    modern: bool,

    #[arg(
        long,
        env = "COMPAT_RELEASE",
        value_name = "RELEASE",
        hide = true,
        conflicts_with = "modern"
    )]
    compat_release: Option<String>,

    #[arg(
        long,
        env = "UPGRADE_RELEASE",
        value_name = "RELEASE",
        hide = true,
        default_value = "oldstable"
    )]
    upgrade_release: Option<String>,

    #[arg(long, hide = true)]
    minimum_certainty: Option<Certainty>,

    #[arg(long, hide = true, default_value_t = true)]
    opinionated: bool,

    #[arg(long, hide = true, default_value_t = 0, value_name = "DILIGENCE")]
    diligent: i32,

    /// Include changes with lower certainty
    #[arg(long, default_value_t = false)]
    uncertain: bool,

    #[arg(long, default_value_t = false, hide = true)]
    yolo: bool,

    #[arg(long, default_value_t = false, hide = true)]
    force_subprocess: bool,
}

#[derive(clap::Args, Clone, Debug)]
#[group()]
struct PackageArgs {
    /// Allow file reformatting and stripping of comments
    #[arg(short, long)]
    allow_reformatting: Option<bool>,

    /// Whether to trust the package
    #[arg(long, default_value_t = false, hide = true)]
    trust: bool,
}

#[derive(clap::Args, Clone, Debug)]
#[group()]
struct OutputArgs {
    /// Be verbose
    #[arg(short, long, default_value_t = std::env::var("SVP_API").is_ok())]
    verbose: bool,

    /// Print resulting diff afterwards
    #[arg(long, default_value_t = false)]
    diff: bool,

    /// Enable debug output
    #[arg(long, default_value_t = false)]
    debug: bool,

    /// List available fixers
    #[arg(
        long,
        default_value_t = false,
        conflicts_with = "list_tags",
        conflicts_with = "identity"
    )]
    list_fixers: bool,

    /// List lintian tags for which fixers are available
    #[arg(
        long,
        default_value_t = false,
        conflicts_with = "list_fixers",
        conflicts_with = "identity"
    )]
    list_tags: bool,

    /// Do not make any changes to the current repository.
    /// Note: currently creates a temporary clone of the repository.
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Print user identity that would be used when committing
    #[arg(
        long,
        default_value_t = false,
        conflicts_with = "list_fixers",
        conflicts_with = "list_tags"
    )]
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

    /// Display statistics on fixer performance
    #[arg(long, default_value_t = false)]
    stats: bool,
}

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    #[command(flatten)]
    fixers: FixerArgs,

    #[command(flatten)]
    packages: PackageArgs,

    #[command(flatten)]
    output: OutputArgs,
}

fn main() -> Result<(), i32> {
    let args = Args::parse();

    // Create MultiProgress for coordinating progress bars with logging
    let multi_progress = indicatif::MultiProgress::new();

    // Set up tracing subscriber with a custom writer that suspends progress bars
    use tracing_subscriber::fmt::format::Writer;
    use tracing_subscriber::fmt::FmtContext;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::registry::LookupSpan;
    use tracing_subscriber::util::SubscriberInitExt;

    struct ProgressSuspendingWriter {
        multi_progress: indicatif::MultiProgress,
    }

    impl std::io::Write for ProgressSuspendingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.multi_progress.suspend(|| std::io::stderr().write(buf))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.multi_progress.suspend(|| std::io::stderr().flush())
        }
    }

    // Store span data for later retrieval
    #[derive(Debug)]
    struct FixerSpanData {
        name: String,
    }

    // Custom format that shows fixer name in brackets
    struct FixerFormat;

    impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for FixerFormat
    where
        S: tracing::Subscriber + for<'a> LookupSpan<'a>,
        N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
    {
        fn format_event(
            &self,
            ctx: &FmtContext<'_, S, N>,
            mut writer: Writer<'_>,
            event: &tracing::Event<'_>,
        ) -> std::fmt::Result {
            // Look for a fixer span in the current context (if any)
            if let Some(span_ref) = ctx.event_scope() {
                for span in span_ref {
                    if span.name() == "fixer" {
                        let extensions = span.extensions();
                        if let Some(data) = extensions.get::<FixerSpanData>() {
                            // Use dim style for subtle visual distinction
                            use nu_ansi_term::Style;
                            write!(writer, "{}: ", Style::new().dimmed().paint(&data.name))?;
                        }
                        break;
                    }
                }
            }

            // Write the message
            ctx.field_format().format_fields(writer.by_ref(), event)?;
            writeln!(writer)
        }
    }

    // Layer to capture span fields
    use tracing::field::{Field, Visit};

    struct FixerLayer;

    impl<S> tracing_subscriber::Layer<S> for FixerLayer
    where
        S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    {
        fn on_new_span(
            &self,
            attrs: &tracing::span::Attributes<'_>,
            id: &tracing::span::Id,
            ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if attrs.metadata().name() == "fixer" {
                let span = ctx.span(id).expect("Span not found");
                let mut visitor = FixerVisitor { name: None };
                attrs.record(&mut visitor);
                if let Some(name) = visitor.name {
                    span.extensions_mut().insert(FixerSpanData { name });
                }
            }
        }
    }

    struct FixerVisitor {
        name: Option<String>,
    }

    impl Visit for FixerVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "name" {
                self.name = Some(value.to_string());
            }
        }

        fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {
            // No-op for other field types
        }
    }

    let filter_level = if args.output.debug {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    let mp_for_writer = multi_progress.clone();
    tracing_subscriber::registry()
        .with(FixerLayer)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(move || ProgressSuspendingWriter {
                    multi_progress: mp_for_writer.clone(),
                })
                .event_format(FixerFormat),
        )
        .with(tracing_subscriber::filter::LevelFilter::from_level(
            filter_level,
        ))
        .init();

    // Set up log forwarding to tracing for compatibility with log:: macros
    tracing_log::LogTracer::init().ok();

    breezyshim::init();

    // TODO(jelmer): Allow changing this via arguments
    let timeout = Some(chrono::Duration::seconds(10));

    let fixers_iter = match lintian_brush::available_lintian_fixers(
        args.fixers.fixers_dir.as_deref(),
        Some(args.fixers.force_subprocess),
    ) {
        Ok(fixers) => fixers,
        Err(e) => {
            tracing::error!("Error loading fixers: {}", e);
            std::process::exit(1);
        }
    };

    let mut fixers: Vec<_> = fixers_iter.collect();

    if args.output.list_fixers {
        fixers.sort_by_key(|a| a.name());
        for fixer in fixers {
            println!("{}", fixer.name());
        }
    } else if args.output.list_tags {
        let tags = fixers
            .iter()
            .flat_map(|f| f.lintian_tags())
            .collect::<std::collections::HashSet<_>>();
        let mut tags: Vec<_> = tags.into_iter().collect();
        tags.sort();
        for tag in tags {
            println!("{}", tag);
        }
    } else {
        let mut update_changelog: Option<bool> = if args.output.update_changelog {
            Some(true)
        } else if args.output.no_update_changelog {
            Some(false)
        } else {
            None
        };

        let mut tempdir = None;

        let (wt, subpath) = if args.output.dry_run {
            let (branch, subpath) = match open_containing_branch(
                &url::Url::from_directory_path(&args.output.directory).unwrap(),
            ) {
                Ok((branch, subpath)) => (branch, subpath),
                Err(Error::NotBranchError(_msg, _)) => {
                    tracing::error!("No version control directory found (e.g. a .git directory).");
                    std::process::exit(1);
                }
                Err(Error::DependencyNotPresent(name, _reason)) => {
                    tracing::error!(
                        "Unable to open branch at {}: missing package {}",
                        args.output.directory.display(),
                        name
                    );
                    std::process::exit(1);
                }
                Err(err) => {
                    tracing::error!(
                        "Unable to open branch at {}: {}",
                        args.output.directory.display(),
                        err
                    );
                    std::process::exit(1);
                }
            };

            let td = match tempfile::tempdir() {
                Ok(td) => td,
                Err(e) => {
                    tracing::error!("Unable to create temporary directory: {}", e);
                    std::process::exit(1);
                }
            };

            // TODO(jelmer): Make a slimmer copy

            let to_dir = match branch.controldir().sprout(
                url::Url::from_directory_path(td.path()).unwrap(),
                None,
                Some(true),
                Some(branch.format().supports_stacking()),
                None,
            ) {
                Ok(to_dir) => to_dir,
                Err(e) => {
                    tracing::error!("Unable to create temporary branch: {}", e);
                    std::process::exit(1);
                }
            };
            tempdir = Some(td);
            (to_dir.open_workingtree().unwrap(), subpath)
        } else {
            match workingtree::open_containing(&args.output.directory) {
                Ok((wt, subpath)) => (wt, subpath.display().to_string()),
                Err(Error::NotBranchError(_msg, _)) => {
                    tracing::error!("No version control directory found (e.g. a .git directory).");
                    std::process::exit(1);
                }
                Err(Error::DependencyNotPresent(name, _reason)) => {
                    tracing::error!(
                        "Unable to open tree at {}: missing package {}",
                        args.output.directory.display(),
                        name
                    );
                    std::process::exit(1);
                }
                Err(e) => {
                    tracing::error!(
                        "Unable to open tree at {}: {}",
                        args.output.directory.display(),
                        e
                    );
                    std::process::exit(1);
                }
            }
        };
        if args.output.identity {
            println!("Committer identity: {}", get_committer(&wt));
            let (maintainer, email) = get_maintainer().unwrap_or(("".to_string(), "".to_string()));
            println!("Changelog identity: {} <{}>", maintainer, email);
            std::process::exit(0);
        }

        let svp = svp_client::Reporter::new(versions_dict());

        let since_revid = wt.last_revision().unwrap();
        if args.fixers.fixers.is_some() || args.fixers.exclude.is_some() {
            let include = args
                .fixers
                .fixers
                .as_ref()
                .map(|fs| fs.iter().map(|f| f.as_str()).collect::<Vec<_>>());
            let exclude = args
                .fixers
                .exclude
                .as_ref()
                .map(|fs| fs.iter().map(|f| f.as_str()).collect::<Vec<_>>());
            fixers =
                match lintian_brush::select_fixers(fixers, include.as_deref(), exclude.as_deref()) {
                    Ok(fixers) => fixers,
                    Err(lintian_brush::UnknownFixer(f)) => {
                        tracing::error!("Unknown fixer specified: {}", f);
                        std::process::exit(1);
                    }
                }
        }
        let debian_info = distro_info::DebianDistroInfo::new().unwrap();
        let mut compat_release = if args.fixers.modern {
            Some(
                debian_info
                    .releases()
                    .iter()
                    .find(|release| release.series() == "sid")
                    .unwrap()
                    .series()
                    .to_string(),
            )
        } else {
            args.fixers.compat_release.clone()
        };
        let upgrade_release: Option<String> =
            if let Some(upgrade_release) = args.fixers.upgrade_release.as_ref() {
                Some(upgrade_release.clone())
            } else if let Some(compat_release) = compat_release.as_ref() {
                // Pick two releases back from the compat release (unstable/sid => stable => oldstable)
                debian_info
                    .releases()
                    .iter()
                    .rev()
                    .filter(|release| release.series() != compat_release)
                    .take(2)
                    .next()
                    .map(|release| release.series().to_string())
            } else {
                None
            };
        let mut minimum_certainty = args.fixers.minimum_certainty;
        let mut allow_reformatting = args.packages.allow_reformatting;
        match debian_analyzer::config::Config::from_workingtree(
            &wt,
            std::path::Path::new(subpath.as_str()),
        ) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::error!("Unable to read config: {}", e);
                std::process::exit(1);
            }
            Ok(cfg) => {
                if minimum_certainty.is_none() {
                    minimum_certainty = cfg.minimum_certainty();
                }
                if compat_release.is_none() {
                    compat_release = cfg.compat_release();
                }
                if allow_reformatting.is_none() {
                    allow_reformatting = cfg.allow_reformatting();
                }
                if update_changelog.is_none() {
                    update_changelog = cfg.update_changelog();
                }
            }
        }
        let minimum_certainty = minimum_certainty.unwrap_or_else(|| {
            if args.fixers.uncertain || args.fixers.yolo {
                Certainty::Possible
            } else {
                Certainty::default()
            }
        });
        let compat_release = compat_release.as_ref().map_or_else(
            || {
                debian_info
                    .released(chrono::Local::now().naive_local().date())
                    .into_iter()
                    .next_back()
                    .unwrap()
                    .series()
                    .to_string()
            },
            |s| s.clone(),
        );

        let upgrade_release = upgrade_release.as_ref().map_or_else(
            || {
                debian_info
                    .released(chrono::Local::now().naive_local().date())
                    .into_iter()
                    .next_back()
                    .unwrap()
                    .series()
                    .to_string()
            },
            |s| s.clone(),
        );

        if args.output.verbose {
            tracing::info!("Using parameters:");
            tracing::info!(" compatibility release: {}", compat_release);
            tracing::info!(" minimum certainty: {}", minimum_certainty);
            if let Some(allow_reformatting) = allow_reformatting {
                tracing::info!(" allow reformatting: {}", allow_reformatting);
            } else {
                tracing::info!(" allow reformatting: auto");
            }
            if let Some(update_changelog) = update_changelog {
                tracing::info!(" update changelog: {}", update_changelog);
            } else {
                tracing::info!(" update changelog: auto");
            }
        }

        let write_lock = wt.lock_write();
        if debian_analyzer::control_files_in_root(&wt, std::path::Path::new(subpath.as_str())) {
            drop(write_lock);
            svp.report_fatal(
                "control-files-in-root",
                "control files live in root rather than debian/ (LarstIQ mode)",
                None,
                Some(false),
            );
        }

        let preferences = lintian_brush::FixerPreferences {
            compat_release: Some(compat_release),
            minimum_certainty: Some(minimum_certainty),
            allow_reformatting,
            net_access: Some(!args.output.disable_net_access),
            opinionated: Some(args.fixers.opinionated),
            diligence: Some(args.fixers.diligent),
            trust_package: Some(args.packages.trust),
            upgrade_release: Some(upgrade_release),
            ..Default::default()
        };

        let mut overall_result = match lintian_brush::run_lintian_fixers(
            &wt,
            fixers.as_slice(),
            update_changelog.as_ref().map(|b| || *b),
            args.output.verbose,
            None,
            &preferences,
            if args.output.disable_inotify {
                Some(false)
            } else {
                None
            },
            Some(std::path::Path::new(subpath.as_str())),
            Some("lintian-brush"),
            timeout,
            Some(&multi_progress),
        ) {
            Err(OverallError::NotDebianPackage(p)) => {
                drop(write_lock);
                svp.report_fatal(
                    "not-debian-package",
                    format!("{}: Not a Debian package", p.display()).as_str(),
                    None,
                    None,
                );
            }
            Err(OverallError::WorkspaceDirty(p)) => {
                drop(write_lock);
                tracing::error!(
                    "{}: Please commit pending changes and remove unknown files first.",
                    p.display()
                );
                if args.output.verbose {
                    breezyshim::status::show_tree_status(&wt).unwrap();
                }
                std::process::exit(1);
            }
            Err(OverallError::ChangelogCreate(e)) => {
                drop(write_lock);
                svp.report_fatal(
                    "changelog-create-error",
                    format!("Error creating changelog entry: {}", e).as_str(),
                    None,
                    None,
                );
            }
            Err(OverallError::InvalidChangelog(p, s)) => {
                drop(write_lock);
                svp.report_fatal(
                    "invalid-changelog",
                    format!("{}: Invalid changelog: {}", p.display(), s).as_str(),
                    None,
                    None,
                );
            }
            #[cfg(feature = "python")]
            Err(OverallError::Python(e)) => {
                drop(write_lock);
                svp.report_fatal(
                    "python-error",
                    format!("Error running Python: {}", e).as_str(),
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
            Err(OverallError::IoError(e)) => {
                drop(write_lock);
                svp.report_fatal("io-error", format!("I/O error: {}", e).as_str(), None, None);
            }
            Err(OverallError::Other(e)) => {
                drop(write_lock);
                svp.report_fatal(
                    "other-error",
                    format!("Other error: {}", e).as_str(),
                    None,
                    None,
                );
            }
            Ok(overall_result) => overall_result,
        };
        std::mem::drop(write_lock);
        if let Some(tempdir) = tempdir {
            if let Err(e) = tempdir.close() {
                tracing::warn!("Error removing temporary directory: {}", e);
            }
        }

        if !overall_result.overridden_lintian_issues.is_empty() {
            if overall_result.overridden_lintian_issues.len() == 1 {
                tracing::info!(
                    "{} change skipped because of lintian overrides.",
                    overall_result.overridden_lintian_issues.len()
                );
            } else {
                tracing::info!(
                    "{} changes skipped because of lintian overrides.",
                    overall_result.overridden_lintian_issues.len()
                );
            }
        }
        if !overall_result.success.is_empty() {
            let all_tags = overall_result.tags_count();
            if !all_tags.is_empty() {
                tracing::info!(
                    "Lintian tags fixed: {:?}",
                    all_tags.keys().collect::<Vec<_>>()
                );
            } else {
                tracing::info!("Some changes were made, but there are no affected lintian tags.");
            }
            let min_certainty = overall_result.minimum_success_certainty();
            if min_certainty != Certainty::Certain {
                tracing::info!(
                    "Some changes were made with lower certainty ({}); please double check the changes.",
                    min_certainty
                );
            }
        } else {
            tracing::info!("No changes made.");
        }
        if !overall_result.failed_fixers.is_empty() && !args.output.verbose {
            tracing::info!("Some fixer scripts failed to run:");
            for (name, reason) in overall_result.failed_fixers.iter() {
                tracing::info!("  {}: {}", name, reason);
            }
            tracing::info!("Run with --verbose for details.");
        }
        if !overall_result.formatting_unpreservable.is_empty() && !args.output.verbose {
            tracing::info!(
                "Some fixer scripts were unable to preserve formatting: {:?}. Run with --allow-reformatting to reformat {:?}.",
                overall_result.formatting_unpreservable.keys().collect::<Vec<_>>(),
                overall_result.formatting_unpreservable.values().collect::<Vec<_>>()
            );
        }
        if args.output.stats {
            tracing::info!("Fixer performance statistics:");

            // Collect all fixers with their durations from the HashMap
            let mut fixer_stats: Vec<_> = overall_result
                .fixer_durations
                .iter()
                .map(|(name, duration)| (name.as_str(), *duration))
                .collect();

            // Sort by duration (slowest first)
            fixer_stats.sort_by(|a, b| b.1.cmp(&a.1));

            // Display statistics
            let total_duration: std::time::Duration = overall_result.fixer_durations.values().sum();

            println!("\n{:<50} {:>12} {:>10}", "Fixer", "Duration (s)", "Result");
            println!("{}", "-".repeat(75));

            // Create a set of successful fixer names for quick lookup
            let successful_fixers: std::collections::HashSet<&str> = overall_result
                .success
                .iter()
                .map(|fs| fs.fixer_name.as_str())
                .collect();

            for (name, duration) in &fixer_stats {
                let result = if successful_fixers.contains(name) {
                    "success"
                } else {
                    "no change"
                };
                println!(
                    "{:<50} {:>12.2} {:>10}",
                    name,
                    duration.as_secs_f32(),
                    result
                );
            }

            println!("{}", "-".repeat(75));
            println!("{:<50} {:>12.2}", "TOTAL", total_duration.as_secs_f32());
            println!(
                "\n{} fixer(s) ran, {} made changes",
                overall_result.fixer_durations.len(),
                overall_result.success.len()
            );
        }
        if args.output.diff {
            breezyshim::diff::show_diff_trees(
                &wt.branch()
                    .repository()
                    .revision_tree(&since_revid)
                    .unwrap(),
                &wt,
                Box::new(std::io::stdout()),
                None,
                None,
            )
            .unwrap();
        }
        if svp.enabled() {
            if let Some(base) = svp.load_resume::<ManyResult>() {
                overall_result.success.extend(base.success);
            }
            let changelog_behaviour = overall_result.changelog_behaviour.clone();
            svp.report_success_debian(
                Some(overall_result.value()),
                Some(overall_result),
                changelog_behaviour.map(|b| b.into()),
            )
        }
    }
    Ok(())
}

fn versions_dict() -> HashMap<String, String> {
    let mut ret = HashMap::new();
    ret.insert(
        "lintian-brush".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    let breezy_version = breezyshim::version::version();
    ret.insert("breezy".to_string(), breezy_version.to_string());
    ret
}
