use breezyshim::tree::WorkingTree;
use clap::Parser;
use std::io::Write as _;

#[derive(Parser)]
#[command(author, version)]
struct Args {
    /// Be verbose
    #[clap(long)]
    verbose: bool,

    /// The directory to check
    #[clap(default_value = ".")]
    directory: std::path::PathBuf,
}

fn main() {
    let args = Args::parse();

    env_logger::builder()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .filter(
            None,
            if args.verbose {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Info
            },
        )
        .init();

    breezyshim::init();

    let (wt, subpath) = WorkingTree::open_containing(&args.directory).unwrap();
    let debian_path = if debian_analyzer::control_files_in_root(&wt, subpath.as_path()) {
        subpath
    } else {
        subpath.join("debian")
    };
    let changelog_behaviour =
        debian_analyzer::detect_gbp_dch::guess_update_changelog(&wt, debian_path.as_path(), None);
    if let Some(changelog_behaviour) = changelog_behaviour {
        log::info!("{}", changelog_behaviour.explanation);
        println!("{}", changelog_behaviour.update_changelog);
    } else {
        log::info!("Unable to determine changelog updating behaviour");
        std::process::exit(1)
    }
}
