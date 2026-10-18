//! The bridge's async run loop.
//!
//! A single long-running `tokio` task: it connects to MQTT, publishes Home
//! Assistant discovery and availability on each (re)connection, services
//! command topics, and publishes device state on a fixed interval. A dropped
//! broker triggers a backoff-and-reconnect rather than a panic; an Envoy or
//! endpoint error is logged and swallowed, never ending the loop.

use core::time::Duration;

use rumqttc::{AsyncClient, ConnectionError, Event, LastWill, MqttOptions, Packet, QoS};
use tracing::{info, warn};

use enphase_api::{Credentials, Entrez, Envoy};

use crate::cli::Args;
use crate::controls::{EnphaseControls, EnvoyControls};
use crate::discovery::discovery_configs;
use crate::dispatch::handle_command;
use crate::payload::{parse_command, state_messages};
use crate::state::collect_state;
use crate::topics::BridgeTopics;

/// The discovery node id embedded in the HA config topics.
const NODE_ID: &str = "enphase_bridge";
/// The initial (and minimum) reconnect backoff.
const MIN_BACKOFF: Duration = Duration::from_secs(1);
/// The maximum reconnect backoff.
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// The MQTT client's in-flight request queue capacity.
const CLIENT_CAPACITY: usize = 16;

/// Compute the next reconnect backoff.
///
/// Doubles the current backoff, capped at [`MAX_BACKOFF`].
///
/// # Arguments
///
/// * `current` - The current backoff duration.
///
/// # Returns
///
/// The next backoff duration.
#[inline]
#[must_use]
pub fn next_backoff(current: Duration) -> Duration {
    current.saturating_mul(2).min(MAX_BACKOFF)
}

/// Publish discovery and availability, and subscribe to command topics.
///
/// Runs on each (re)connection. Grid-profile options are best-effort: if the
/// device is unreachable the discovery is published with no options rather than
/// failing the connection.
///
/// # Arguments
///
/// * `client` - The connected MQTT client.
/// * `controls` - The device controls, for reading grid-profile options.
/// * `topics` - The bridge's topic layout.
///
/// # Errors
///
/// Returns an error if an MQTT publish or subscribe fails.
async fn announce(
    client: &AsyncClient,
    controls: &impl EnphaseControls,
    topics: &BridgeTopics,
) -> anyhow::Result<()> {
    publish_discovery(client, controls, topics).await?;

    for topic in [
        &topics.panels_command,
        &topics.zero_export_command,
        &topics.export_limit_command,
        &topics.grid_profile_command,
    ] {
        client.subscribe(topic.clone(), QoS::AtLeastOnce).await?;
    }
    Ok(())
}

/// Publish availability `online` and the HA discovery configs.
///
/// Grid-profile options are best-effort: if the device is unreachable the
/// discovery is published with no options rather than failing. This is also run
/// on each poll (not only at connect), so the `select` entity's options
/// populate once the device becomes reachable even if it was down at connect.
///
/// # Arguments
///
/// * `client` - The connected MQTT client.
/// * `controls` - The device controls, for reading grid-profile options.
/// * `topics` - The bridge's topic layout.
///
/// # Errors
///
/// Returns an error if an MQTT publish fails.
async fn publish_discovery(
    client: &AsyncClient,
    controls: &impl EnphaseControls,
    topics: &BridgeTopics,
) -> anyhow::Result<()> {
    client
        .publish(
            topics.availability.clone(),
            QoS::AtLeastOnce,
            true,
            "online",
        )
        .await?;

    let profile_options = match controls.grid_profiles().await {
        Ok(profiles) => profiles.available,
        Err(err) => {
            warn!("grid profiles unavailable for discovery: {err:#}");
            Vec::new()
        }
    };
    for message in discovery_configs(topics, NODE_ID, &profile_options) {
        client
            .publish(message.topic, QoS::AtLeastOnce, true, message.payload)
            .await?;
    }
    Ok(())
}

/// Collect and publish the current device state.
///
/// Both collection and publishing are best-effort; failures are logged, not
/// propagated, so one bad poll never ends the loop.
///
/// # Arguments
///
/// * `client` - The connected MQTT client.
/// * `controls` - The device controls to read.
/// * `topics` - The bridge's topic layout.
async fn publish_state(
    client: &AsyncClient,
    controls: &impl EnphaseControls,
    topics: &BridgeTopics,
) {
    match collect_state(controls).await {
        Ok(state) => {
            for (topic, payload) in state_messages(topics, &state) {
                if let Err(err) = client.publish(topic, QoS::AtLeastOnce, true, payload).await {
                    warn!("state publish failed: {err}");
                }
            }
        }
        Err(err) => warn!("state collection failed: {err:#}"),
    }
}

/// Read a required configuration value, or report which one is missing.
///
/// # Arguments
///
/// * `value` - The optional argument value.
/// * `flag` - The CLI flag name, for the error message.
/// * `env` - The environment variable name, for the error message.
///
/// # Returns
///
/// The value as a string slice.
///
/// # Errors
///
/// Returns an error naming the missing flag and environment variable.
fn require<'arg>(value: Option<&'arg str>, flag: &str, env: &str) -> anyhow::Result<&'arg str> {
    value.ok_or_else(|| anyhow::anyhow!("missing required configuration: --{flag} (or {env})"))
}

/// Build the self-authenticating Envoy controls from the CLI arguments.
///
/// # Arguments
///
/// * `args` - The parsed CLI arguments.
///
/// # Returns
///
/// The device controls.
///
/// # Errors
///
/// Returns an error if any required Enphase identity argument is missing.
fn build_controls(args: &Args) -> anyhow::Result<EnvoyControls> {
    let credentials = Credentials::new(
        require(args.username.as_deref(), "username", "ENPHASE_USERNAME")?,
        require(args.password.as_deref(), "password", "ENPHASE_PASSWORD")?,
        require(args.site_name.as_deref(), "site-name", "ENPHASE_SITE_NAME")?,
        require(
            args.serial_number.as_deref(),
            "serial-number",
            "ENPHASE_SERIAL_NUMBER",
        )?,
    )
    .commissioned(args.commissioned);

    let host = require(args.host.as_deref(), "host", "ENVOY_HOST")?;
    let mut envoy = Envoy::with_auth(host, Entrez::default(), credentials);
    if let Some(path) = &args.token_cache {
        envoy = envoy.with_token_cache(path.clone());
    }
    Ok(EnvoyControls::new(envoy))
}

/// Build the MQTT options, including the availability Last Will.
///
/// # Arguments
///
/// * `args` - The parsed CLI arguments.
/// * `topics` - The bridge's topic layout (for the Last Will topic).
///
/// # Returns
///
/// The configured MQTT options.
fn build_mqtt_options(args: &Args, topics: &BridgeTopics) -> MqttOptions {
    let mut options = MqttOptions::new(
        args.client_id.clone(),
        args.broker_host.clone(),
        args.broker_port,
    );
    options.set_keep_alive(Duration::from_secs(30));
    options.set_last_will(LastWill::new(
        topics.availability.clone(),
        "offline",
        QoS::AtLeastOnce,
        true,
    ));
    match (&args.mqtt_username, &args.mqtt_password) {
        (Some(username), Some(password)) => {
            options.set_credentials(username.clone(), password.clone());
        }
        (Some(_), None) | (None, Some(_)) => {
            warn!("only one of MQTT username/password provided; connecting anonymously");
        }
        (None, None) => {}
    }
    options
}

/// Handle one polled MQTT event.
///
/// A fresh connection republishes discovery and availability; an incoming
/// publish is parsed and dispatched; a connection error backs off and lets the
/// event loop reconnect. Control errors are logged, never propagated.
///
/// # Arguments
///
/// * `event` - The event (or error) from the MQTT event loop.
/// * `client` - The connected MQTT client.
/// * `controls` - The device controls.
/// * `topics` - The bridge's topic layout.
/// * `backoff` - The current reconnect backoff, updated in place.
#[expect(
    clippy::cognitive_complexity,
    reason = "a flat dispatch over the event loop's packet variants"
)]
async fn handle_poll_event(
    event: Result<Event, ConnectionError>,
    client: &AsyncClient,
    controls: &impl EnphaseControls,
    topics: &BridgeTopics,
    backoff: &mut Duration,
) {
    match event {
        Ok(Event::Incoming(Packet::ConnAck(_))) => {
            *backoff = MIN_BACKOFF;
            info!("connected to MQTT broker; publishing discovery");
            if let Err(err) = announce(client, controls, topics).await {
                warn!("on-connect announce failed: {err:#}");
            }
        }
        Ok(Event::Incoming(Packet::Publish(publish))) => {
            if let Some(command) = parse_command(&publish.topic, &publish.payload, topics)
                && let Err(err) = handle_command(command, controls).await
            {
                warn!("command on {} failed: {err:#}", publish.topic);
            }
        }
        Ok(_) => {}
        Err(err) => {
            warn!("MQTT connection error: {err}; reconnecting in {backoff:?}");
            tokio::time::sleep(*backoff).await;
            *backoff = next_backoff(*backoff);
        }
    }
}

/// Run the bridge until a shutdown signal is received.
///
/// Connects to MQTT and drives the event loop: each connection republishes
/// discovery and availability, incoming commands are dispatched to the device,
/// and state is published on the poll interval. Connection errors back off and
/// reconnect; control errors are logged.
///
/// # Arguments
///
/// * `args` - The parsed CLI arguments.
///
/// # Errors
///
/// Returns an error if a required Enphase identity argument is missing (before
/// the loop starts). The run loop itself never propagates a transient error:
/// broker, Envoy, and shutdown-publish failures are logged, not returned.
#[inline]
#[expect(
    clippy::integer_division_remainder_used,
    clippy::cognitive_complexity,
    reason = "both live inside the tokio::select! macro expansion"
)]
pub async fn run(args: Args) -> anyhow::Result<()> {
    let topics = BridgeTopics::new(&args.topic_prefix);
    let controls = build_controls(&args)?;
    let options = build_mqtt_options(&args, &topics);

    let (client, mut eventloop) = AsyncClient::new(options, CLIENT_CAPACITY);

    let mut interval = tokio::time::interval(Duration::from_secs(args.poll_interval));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut backoff = MIN_BACKOFF;
    info!("enphase-bridge starting; connecting to MQTT broker");

    loop {
        tokio::select! {
            event = eventloop.poll() => {
                handle_poll_event(event, &client, &controls, &topics, &mut backoff).await;
            }
            _ = interval.tick() => {
                // Re-publish discovery (retained, idempotent) so the grid-profile
                // select's options populate once the device is reachable, even if
                // it was down at connect time.
                if let Err(err) = publish_discovery(&client, &controls, &topics).await {
                    warn!("periodic discovery refresh failed: {err}");
                }
                publish_state(&client, &controls, &topics).await;
            }
            signal = tokio::signal::ctrl_c() => {
                if let Err(err) = signal {
                    warn!("failed to listen for shutdown signal: {err}");
                }
                info!("shutdown signal received; marking bridge offline");
                if let Err(err) = client
                    .publish(topics.availability.clone(), QoS::AtLeastOnce, true, "offline")
                    .await
                {
                    warn!("offline-availability publish failed: {err}");
                }
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use core::time::Duration;

    use pretty_assertions::assert_eq;

    #[test]
    fn next_backoff_doubles_and_caps() {
        assert_eq!(next_backoff(Duration::from_secs(1)), Duration::from_secs(2));
        assert_eq!(
            next_backoff(Duration::from_secs(40)),
            Duration::from_secs(60)
        );
    }
}
