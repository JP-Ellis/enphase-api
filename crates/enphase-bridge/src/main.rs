//! Enphase ↔ MQTT bridge binary.
//!
//! A thin shim over the [`enphase_bridge`] library: it parses arguments, sets
//! up logging, and either runs the bridge or prints the version.

use std::process::ExitCode;

use clap::Parser as _;
use tracing::error;

use enphase_bridge::cli::{Args, Command};
use enphase_bridge::{bridge, logging, version};

/// Entry point.
///
/// Parses arguments, initialises logging, and dispatches to the bridge run loop
/// or the `version` subcommand.
#[tokio::main]
#[expect(
    clippy::print_stdout,
    reason = "the version subcommand prints version information to stdout"
)]
async fn main() -> ExitCode {
    let args = Args::parse();
    logging::setup_tracing(args.verbosity);

    let result = if matches!(args.command, Some(Command::Version)) {
        println!("{}", version::Version::default());
        Ok(())
    } else {
        bridge::run(args).await
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!("{err:#}");
            ExitCode::FAILURE
        }
    }
}
