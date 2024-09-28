use clap::CommandFactory;
use clap::Parser;

#[derive(Parser)]
#[command(author, version)]
struct Args {
    #[clap(long, default_value = "sid")]
    release: String,

    #[clap(long, conflicts_with = "list-uses-transitional-dummy")]
    list_transitional_dummy: bool,

    #[clap(long, conflicts_with = "list-transitional-dummy")]
    list_uses_transitional_dummy: bool,
}

#[tokio::main]
async fn main() -> Result<(), i32> {
    let args = Args::parse();

    env_logger::init();

    let udd = debian_analyzer::udd::connect_udd_mirror().await.unwrap();
    let transitions =
        scrub_obsolete::dummy_transitional::find_dummy_transitional_packages(&udd, &args.release)
            .await
            .unwrap();

    if args.list_transitional_dummy {
        serde_yaml::to_writer(std::io::stdout(), &transitions).unwrap();
        Ok(())
    } else if args.list_uses_transitional_dummy {
        for dep in transitions {
            let by_source =
                scrub_obsolete::dummy_transitional::find_reverse_dependencies(&udd, &dep.0)
                    .await
                    .unwrap();
            for (source, binaries) in by_source {
                for binary in binaries {
                    log::info!("{} / {} / {}", source, binary, dep.0);
                }
            }
        }
        Ok(())
    } else {
        Args::command().print_help().unwrap();
        Err(1)
    }
}
