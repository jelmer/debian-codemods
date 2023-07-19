use breezyshim::tree::WorkingTree;
use clap::Parser;
use std::io::Write as _;

#[derive(Parser)]
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

    pyo3::Python::with_gil(|py| {
        py.import("breezy.bzr").unwrap();
        py.import("breezy.git").unwrap();
    });

    let (wt, subpath) = WorkingTree::open_containing(&args.directory).unwrap();
    let debian_path = if lintian_brush::control_files_in_root(&wt, subpath.as_path()) {
        subpath.to_path_buf()
    } else {
        subpath.join("debian")
    };
    let changelog_behaviour =
        lintian_brush::detect_gbp_dch::guess_update_changelog(&wt, debian_path.as_path(), None);
    if let Some(changelog_behaviour) = changelog_behaviour {
        log::info!("{}", changelog_behaviour.explanation);
        println!("{}", changelog_behaviour.update_changelog);
    } else {
        log::info!("Unable to determine changelog updating behaviour");
        std::process::exit(1)
    }
}
