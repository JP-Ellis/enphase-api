//! Command-line interface.

use std::path::PathBuf;

use crate::version::Version;

/// Parse and validate the export-limit cap: a finite, strictly positive wattage.
///
/// # Arguments
///
/// * `value` - The raw `--export-limit-max` argument.
///
/// # Returns
///
/// The parsed cap in watts.
///
/// # Errors
///
/// Returns a message if the value does not parse, is not finite (`nan`, `inf`),
/// or is not strictly positive — any of which would silently reject every
/// export-limit command or defeat the regulatory cap.
fn parse_export_limit_max(value: &str) -> Result<f64, String> {
    let watts: f64 = value
        .parse()
        .map_err(|_ignored| format!("`{value}` is not a number"))?;
    if watts.is_finite() && watts > 0.0 {
        Ok(watts)
    } else {
        Err(format!(
            "export-limit cap must be a finite, positive wattage, got `{value}`"
        ))
    }
}

/// The `enphase-bridge` command-line arguments.
///
/// With no subcommand the bridge runs; the Enphase identity arguments are then
/// required (each may come from its environment variable) and validated at
/// startup. A subcommand such as `version` needs none of them.
#[derive(clap::Parser, Debug)]
#[command(name = "enphase-bridge")]
#[command(about = "Enphase ↔ MQTT bridge for Home Assistant")]
#[command(version = Version::default())]
#[non_exhaustive]
pub struct Args {
    /// Increase verbosity level: `-v` info, `-vv` debug with timing.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbosity: u8,

    /// Enphase account username, for token acquisition.
    #[arg(long, env = "ENPHASE_USERNAME")]
    pub username: Option<String>,

    /// Enphase account password, for token acquisition.
    #[arg(long, env = "ENPHASE_PASSWORD")]
    pub password: Option<String>,

    /// Envoy host (IP or hostname).
    #[arg(long, env = "ENVOY_HOST")]
    pub host: Option<String>,

    /// The site name the token is generated for.
    #[arg(long, env = "ENPHASE_SITE_NAME")]
    pub site_name: Option<String>,

    /// The Envoy serial number the token is generated for.
    #[arg(long, env = "ENPHASE_SERIAL_NUMBER")]
    pub serial_number: Option<String>,

    /// Whether the system is commissioned (affects token acquisition).
    #[arg(
        long,
        env = "ENPHASE_COMMISSIONED",
        action = clap::ArgAction::Set,
        default_value_t = true
    )]
    pub commissioned: bool,

    /// Optional path to a `0600` on-disk token cache.
    #[arg(long, env = "ENPHASE_TOKEN_CACHE")]
    pub token_cache: Option<PathBuf>,

    /// MQTT broker host.
    #[arg(long, env = "MQTT_BROKER_HOST", default_value = "localhost")]
    pub broker_host: String,

    /// MQTT broker port.
    #[arg(long, env = "MQTT_BROKER_PORT", default_value_t = 1883)]
    pub broker_port: u16,

    /// Optional MQTT username.
    #[arg(long, env = "MQTT_USERNAME")]
    pub mqtt_username: Option<String>,

    /// Optional MQTT password.
    #[arg(long, env = "MQTT_PASSWORD")]
    pub mqtt_password: Option<String>,

    /// MQTT client id.
    #[arg(long, env = "MQTT_CLIENT_ID", default_value = "enphase-bridge")]
    pub client_id: String,

    /// MQTT topic prefix for the bridge's command/state topics.
    #[arg(long, env = "MQTT_TOPIC_PREFIX", default_value = "enphase")]
    pub topic_prefix: String,

    /// State poll interval, in seconds (must be at least 1).
    #[arg(
        long,
        env = "POLL_INTERVAL",
        default_value_t = 30,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub poll_interval: u64,

    /// The export-limit number's maximum, in watts (the owner's regulatory cap).
    #[arg(
        long,
        env = "EXPORT_LIMIT_MAX",
        default_value_t = crate::discovery::DEFAULT_EXPORT_LIMIT_MAX,
        value_parser = parse_export_limit_max
    )]
    pub export_limit_max: f64,

    /// Attempts to apply a command before reverting to last-known-good.
    #[arg(
        long,
        env = "COMMAND_RETRIES",
        default_value_t = 2,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub command_retries: u32,

    /// Consecutive failed polls before an entity is marked unavailable.
    #[arg(
        long,
        env = "POLL_FAILURE_THRESHOLD",
        default_value_t = 3,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub poll_failure_threshold: u32,

    /// Optional subcommand; absent means "run the bridge".
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// The bridge's subcommands.
#[derive(clap::Subcommand, Debug)]
#[non_exhaustive]
pub enum Command {
    /// Print version information and exit.
    Version,
}

#[cfg(test)]
mod tests {
    use super::*;

    use clap::Parser as _;

    use pretty_assertions::assert_eq;

    #[test]
    fn cli_parses_defaults() {
        let args = Args::try_parse_from([
            "enphase-bridge",
            "--username",
            "user",
            "--password",
            "pass",
            "--host",
            "envoy.local",
            "--site-name",
            "Site",
            "--serial-number",
            "121212121212",
        ])
        .expect("args parse");
        assert!(args.command.is_none());
        assert_eq!(args.poll_interval, 30);
        assert_eq!(args.topic_prefix, "enphase");
    }

    #[test]
    fn cli_parses_resilience_knobs() {
        let args = Args::try_parse_from([
            "enphase-bridge",
            "--username",
            "user",
            "--password",
            "pass",
            "--host",
            "envoy.local",
            "--site-name",
            "Site",
            "--serial-number",
            "121212121212",
        ])
        .expect("args parse");
        assert!((args.export_limit_max - 5_000.0_f64).abs() < f64::EPSILON);
        assert_eq!(args.command_retries, 2);
        assert_eq!(args.poll_failure_threshold, 3);
    }

    #[test]
    fn export_limit_max_rejects_non_finite_and_non_positive() {
        for bad in ["nan", "inf", "-1", "0"] {
            let result = Args::try_parse_from([
                "enphase-bridge",
                "--username",
                "user",
                "--password",
                "pass",
                "--host",
                "envoy.local",
                "--site-name",
                "Site",
                "--serial-number",
                "121212121212",
                "--export-limit-max",
                bad,
            ]);
            let _err = result.expect_err("non-finite or non-positive cap is rejected");
        }
    }

    #[test]
    fn export_limit_max_accepts_a_positive_cap() {
        let args = Args::try_parse_from([
            "enphase-bridge",
            "--username",
            "user",
            "--password",
            "pass",
            "--host",
            "envoy.local",
            "--site-name",
            "Site",
            "--serial-number",
            "121212121212",
            "--export-limit-max",
            "3000",
        ])
        .expect("args parse");
        assert!((args.export_limit_max - 3_000.0_f64).abs() < f64::EPSILON);
    }

    #[test]
    fn poll_failure_threshold_rejects_zero() {
        let result = Args::try_parse_from([
            "enphase-bridge",
            "--username",
            "user",
            "--password",
            "pass",
            "--host",
            "envoy.local",
            "--site-name",
            "Site",
            "--serial-number",
            "121212121212",
            "--poll-failure-threshold",
            "0",
        ]);
        let _err = result.expect_err("zero threshold is rejected");
    }

    #[test]
    fn commissioned_flag_accepts_false() {
        let args = Args::try_parse_from([
            "enphase-bridge",
            "--username",
            "user",
            "--password",
            "pass",
            "--host",
            "envoy.local",
            "--site-name",
            "Site",
            "--serial-number",
            "121212121212",
            "--commissioned",
            "false",
        ])
        .expect("args parse");
        assert!(!args.commissioned);
    }
}
