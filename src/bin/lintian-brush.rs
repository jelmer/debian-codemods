use clap::Parser;

#[derive(clap::Args, Clone, Debug)]
#[group()]
struct FixerArgs {
    /// Specific fixers to run
    fixers: Option<Vec<String>>,

    /// Path to fixer scripts
    #[arg(short, long, default_value_t = lintian_brush::find_fixers_dir().unwrap().display().to_string(), value_name="DIR")]
    fixers_dir: String,

    /// Exclude fixers
    #[arg(long, value_name = "EXCLUDE")]
    exclude: Option<Vec<String>>,

    /// Use features/compatibility levels that are not available in stable. (makes backporting
    /// harder)
    #[arg(long)]
    modern: bool,

    #[arg(long, env = "COMPAT_RELEASE", value_name = "RELEASE", hide = true)]
    compat_release: Option<String>,

    #[arg(long, hide = true)]
    minimum_certainty: Option<lintian_brush::Certainty>,

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
    #[arg(short, long, default_value_t = false)]
    allow_reformatting: bool,

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
    #[arg(short, long, default_value_t = std::env::current_dir().unwrap().display().to_string(), value_name = "DIR")]
    directory: String,

    /// Do not probe external services
    #[arg(long, default_value_t = false)]
    disable_net_access: bool,

    /// Disable inotify
    #[arg(long, default_value_t = false, hide = true)]
    disable_inotify: bool,

    #[arg(long, default_value_t = false, conflicts_with = "no_update_changelog")]
    update_changelog: bool,

    #[arg(long, default_value_t = false, conflicts_with = "update_changelog")]
    no_update_changelog: bool,
}

#[derive(Parser, Debug)]
#[clap(name = "lintian-brush", author = "Jelmer Vernooĳ <jelmer@debian.org>")]
#[command(author, version)]
struct Args {
    #[command(flatten)]
    fixers: FixerArgs,

    #[command(flatten)]
    packages: PackageArgs,

    #[command(flatten)]
    output: OutputArgs,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if args.output.verbose {
        env_logger::init();
    } else {
        env_logger::builder()
            .filter(None, log::LevelFilter::Info)
            .init();
    }

    let mut fixers: Vec<_> = lintian_brush::available_lintian_fixers(
        Some(std::path::PathBuf::from(args.fixers.fixers_dir).as_path()),
        Some(args.fixers.force_subprocess),
    )?
    .collect();

    if args.output.list_fixers {
        fixers.sort_by(|a, b| a.name().cmp(b.name()));
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
        let update_changelog: Option<bool> = if args.output.update_changelog {
            Some(true)
        } else if args.output.no_update_changelog {
            Some(false)
        } else {
            None
        };
        let r = pyo3::Python::with_gil(|py| {
            let m = py.import("lintian_brush.__main__")?;
            let main = m.getattr("main")?;
            let kwargs = pyo3::types::PyDict::new(py);
            kwargs.set_item("dry_run", args.output.dry_run)?;
            kwargs.set_item("allow_reformatting", args.packages.allow_reformatting)?;
            kwargs.set_item("trust", args.packages.trust)?;
            kwargs.set_item("verbose", args.output.verbose)?;
            kwargs.set_item("diff", args.output.diff)?;
            kwargs.set_item("disable_net_access", args.output.disable_net_access)?;
            kwargs.set_item("disable_inotify", args.output.disable_inotify)?;
            kwargs.set_item("modern", args.fixers.modern)?;
            kwargs.set_item(
                "minimum_certainty",
                args.fixers.minimum_certainty.map(|x| x.to_string()),
            )?;
            kwargs.set_item("opinionated", args.fixers.opinionated)?;
            kwargs.set_item("diligence", args.fixers.diligent)?;
            kwargs.set_item("uncertain", args.fixers.uncertain)?;
            kwargs.set_item("yolo", args.fixers.yolo)?;
            kwargs.set_item("identity", args.output.identity)?;
            kwargs.set_item("update_changelog", update_changelog)?;
            kwargs.set_item("compat_release", args.fixers.compat_release)?;
            kwargs.set_item("exclude", args.fixers.exclude)?;
            kwargs.set_item("include", args.fixers.fixers)?;
            kwargs.set_item("directory", args.output.directory)?;
            main.call(
                (fixers
                    .into_iter()
                    .map(|f| lintian_brush::py::Fixer(f))
                    .collect::<Vec<_>>(),),
                Some(kwargs),
            )?
            .extract::<Option<i32>>()
        });

        match r {
            Ok(Some(exit_code)) => std::process::exit(exit_code),
            Ok(None) => std::process::exit(0),
            Err(e) => {
                eprintln!("Error: {}", e);
                if args.output.debug {
                    pyo3::Python::with_gil(|py| {
                        if let Some(traceback) = e.traceback(py) {
                            println!("{}", traceback.format().unwrap());
                        }
                    });
                }
                std::process::exit(1);
            }
        }
    }
    Ok(())
}
