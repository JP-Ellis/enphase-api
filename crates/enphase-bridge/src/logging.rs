//! Logging setup for the CLI.

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _};

/// Initializes tracing/logging for the CLI based on the specified verbosity
/// level.
///
/// Sets up the global tracing subscriber with a filter and formatting layer.
/// The `verbosity` argument controls the log level and whether detailed timing
/// information is included:
///
/// - `0`: Only warnings and errors are logged.
/// - `1`: Info, warnings, and errors are logged.
/// - `2` or higher: Debug, info, warnings, and errors are logged, with detailed
///   timing and span events.
///
/// # Arguments
///
/// * `verbosity` - The verbosity level for logging output.
///     - `0`: Warn/error only.
///     - `1`: Info and above.
///     - `2` or higher: Debug and above, with timing and span events.
#[inline]
pub fn setup_tracing(verbosity: u8) {
    let filter = match verbosity {
        0 => EnvFilter::new("warn"),
        1 => EnvFilter::new("info"),
        _ => EnvFilter::new("debug"),
    };

    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_line_number(true)
        .with_writer(std::io::stderr);

    // For -vv and above, enable detailed timing information
    if verbosity >= 2 {
        tracing_subscriber::registry()
            .with(filter)
            .with(
                fmt_layer
                    .with_timer(fmt::time::uptime())
                    .with_span_events(fmt::format::FmtSpan::ENTER | fmt::format::FmtSpan::CLOSE),
            )
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .init();
    }

    tracing::debug!("Tracing initialized with verbosity level {}", verbosity);
}
