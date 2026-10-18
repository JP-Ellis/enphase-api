//! Enphase ↔ MQTT bridge.
//!
//! A thin, long-running process that exposes the Enphase gateway's controls as
//! Home Assistant entities over MQTT discovery. It holds no pricing or decision
//! logic: Home Assistant owns those and drives the bridge through MQTT command
//! topics.

use std::process::ExitCode;

use clap::Parser as _;

mod logging;
mod version;

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
