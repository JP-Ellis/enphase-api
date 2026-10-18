//! Enphase ↔ MQTT bridge binary.
//!
//! A thin shim over the [`enphase_bridge`] library: it parses arguments, sets
//! up logging, and (in a later task) runs the bridge or prints the version.

use std::process::ExitCode;

use clap::Parser as _;

use enphase_bridge::{logging, version};

/// Command-line arguments.
///
/// This is a minimal skeleton fleshed out in a later task with the full bridge
/// configuration and the `version` subcommand.
#[derive(clap::Parser, Debug)]
#[command(name = "enphase-bridge")]
#[command(about = "Enphase ↔ MQTT bridge for Home Assistant")]
#[command(version = version::Version::default())]
struct Args {
    /// Increase verbosity level: `-v` info, `-vv` debug with timing.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbosity: u8,
}

/// Entry point.
///
/// Parses arguments, initialises logging, and returns a process exit code.
#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    logging::setup_tracing(args.verbosity);
    ExitCode::SUCCESS
}
