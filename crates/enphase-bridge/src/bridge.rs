//! The bridge's async run loop.
//!
//! Two cooperating tasks run on a `LocalSet` (the library's [`Envoy`] is
//! `!Send`): an **MQTT driver** that drives `eventloop.poll()` continuously and
//! forwards connection events and parsed commands over an `mpsc` channel, and a
//! **device worker** that owns the [`EnphaseControls`] and the [`StateCache`],
//! services the channel and a poll interval, and publishes. Because device I/O
//! lives in a different task from `poll()`, a slow Envoy can no longer starve the
//! MQTT keep-alive. A dropped broker triggers a backoff-and-reconnect inside the
//! driver; an Envoy or endpoint error is folded into the cache's availability,
//! never ending the loop.

use core::time::Duration;

use async_trait::async_trait;
use rumqttc::{AsyncClient, Event, LastWill, MqttOptions, Packet, QoS};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;
use tracing::{info, warn};

use enphase_api::{Credentials, Entrez, Envoy};

use crate::cache::{PublishMessage, StateCache};
use crate::cli::Args;
use crate::controls::{EnphaseControls, EnvoyControls};
use crate::discovery::{discovery_configs, zero_export_discovery_clear};
use crate::payload::{Command, parse_command};
use crate::state::{apply_command, poll_once};
use crate::topics::BridgeTopics;

/// The publish seam the device worker writes through.
///
/// Behind a trait so the worker's publish behaviour — above all, that it stays
/// silent while the broker is disconnected — can be unit-tested with a mock,
/// without a live broker. [`AsyncClient`] is the real implementation; every
/// publish is `QoS::AtLeastOnce`.
#[cfg_attr(test, mockall::automock)]
#[async_trait(?Send)]
pub(crate) trait MqttPublisher {
    /// Publish one message.
    ///
    /// # Arguments
    ///
    /// * `topic` - The topic to publish to.
    /// * `retain` - Whether the broker retains the message.
    /// * `payload` - The payload to publish.
    ///
    /// # Errors
    ///
    /// Returns an error if the client rejects the publish.
    async fn publish(&self, topic: String, retain: bool, payload: String) -> anyhow::Result<()>;
}

#[async_trait(?Send)]
impl MqttPublisher for AsyncClient {
    #[inline]
    async fn publish(&self, topic: String, retain: bool, payload: String) -> anyhow::Result<()> {
        AsyncClient::publish(self, topic, QoS::AtLeastOnce, retain, payload).await?;
        Ok(())
    }
}

/// The discovery node id embedded in the HA config topics.
const NODE_ID: &str = "enphase_bridge";
/// The initial (and minimum) reconnect backoff.
const MIN_BACKOFF: Duration = Duration::from_secs(1);
/// The maximum reconnect backoff.
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// The MQTT client's in-flight request queue capacity.
///
/// The driver task drives `eventloop.poll()` continuously, so the queue drains
/// even while the device worker is mid-publish; this sizing just absorbs a
/// connection-time burst (availability + offline reset + discovery + state).
const CLIENT_CAPACITY: usize = 256;
/// The capacity of the driver → worker command channel.
const COMMAND_CAPACITY: usize = 64;

/// A message from the MQTT driver to the device worker.
enum WorkerMsg {
    /// The broker connection was (re)established.
    Connected,
    /// The broker connection was lost; the worker must stop publishing until the
    /// next [`Connected`](WorkerMsg::Connected), or its publishes pile up in the
    /// client queue that `poll()` cannot drain while disconnected.
    Disconnected,
    /// A command arrived on a command topic.
    Command(Command),
}

/// The tunable resilience knobs the device worker runs with.
#[derive(Debug, Clone, Copy)]
struct ResilienceConfig {
    /// Seconds between state polls.
    poll_interval: u64,
    /// Consecutive poll failures before an entity goes offline.
    failure_threshold: u32,
    /// Attempts per command before reverting.
    command_retries: u32,
    /// The export-limit `number`'s maximum, in watts.
    export_limit_max: f64,
}

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

/// Build the messages that reset every entity to offline on (re)connect.
///
/// The per-entity availability topics are retained, so a reconnect after a crash
/// would otherwise leave the stale `online` from before the crash — the
/// reachability lie this revision forbids. Published before the first poll, which
/// then brings the reachable entities back online.
///
/// # Arguments
///
/// * `topics` - The bridge's topic layout.
///
/// # Returns
///
/// The reachability-off and three per-entity-offline messages, all retained.
#[inline]
#[must_use]
pub fn offline_reset_messages(topics: &BridgeTopics) -> Vec<PublishMessage> {
    vec![
        PublishMessage {
            topic: topics.reachability_state.clone(),
            payload: "OFF".to_owned(),
            retain: true,
        },
        PublishMessage {
            topic: topics.panels_availability.clone(),
            payload: "offline".to_owned(),
            retain: true,
        },
        PublishMessage {
            topic: topics.export_limit_availability.clone(),
            payload: "offline".to_owned(),
            retain: true,
        },
        PublishMessage {
            topic: topics.grid_profile_availability.clone(),
            payload: "offline".to_owned(),
            retain: true,
        },
    ]
}

/// Publish a batch of messages, logging per-message failures.
///
/// # Arguments
///
/// * `client` - The connected MQTT client.
/// * `messages` - The messages to publish.
async fn publish_all(client: &impl MqttPublisher, messages: Vec<PublishMessage>) {
    for message in messages {
        if let Err(err) = client
            .publish(message.topic, message.retain, message.payload)
            .await
        {
            warn!("publish failed: {err}");
        }
    }
}

/// Publish the Home Assistant discovery configs, logging failures.
///
/// # Arguments
///
/// * `client` - The connected MQTT client.
/// * `topics` - The bridge's topic layout.
/// * `profile_options` - The grid-profile ids for the `select` entity.
/// * `export_limit_max` - The export-limit `number`'s maximum, in watts.
async fn publish_discovery(
    client: &impl MqttPublisher,
    topics: &BridgeTopics,
    profile_options: &[String],
    export_limit_max: f64,
) {
    for message in discovery_configs(topics, NODE_ID, profile_options, export_limit_max) {
        if let Err(err) = client.publish(message.topic, true, message.payload).await {
            warn!("discovery publish failed: {err}");
        }
    }
}

/// Publish the bridge-liveness availability, logging failures.
///
/// # Arguments
///
/// * `client` - The connected MQTT client.
/// * `topics` - The bridge's topic layout.
/// * `payload` - `online` or `offline`.
async fn publish_availability(client: &impl MqttPublisher, topics: &BridgeTopics, payload: &str) {
    if let Err(err) = client
        .publish(topics.availability.clone(), true, payload.to_owned())
        .await
    {
        warn!("availability publish failed: {err}");
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

/// Complete when either `ctrl_c` (SIGINT) or SIGTERM fires.
///
/// Created once and pinned for the loop's lifetime, so the signal registration is
/// not torn down and rebuilt each iteration. If the SIGTERM handler cannot be
/// installed, it falls back to `ctrl_c` alone.
#[expect(
    clippy::integer_division_remainder_used,
    reason = "lives inside the tokio::select! macro expansion"
)]
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            warn!("failed to listen for ctrl_c: {err}");
        }
    };
    match signal(SignalKind::terminate()) {
        Ok(mut terminate) => {
            tokio::select! {
                () = ctrl_c => {},
                _ = terminate.recv() => {},
            }
        }
        Err(err) => {
            warn!("SIGTERM handler unavailable: {err}; falling back to ctrl_c only");
            ctrl_c.await;
        }
    }
}

/// Drive the MQTT event loop and forward events to the device worker.
///
/// Loops `eventloop.poll()` without interruption so the keep-alive is never
/// blocked by device I/O. A `ConnAck` subscribes to the command topics and
/// signals [`WorkerMsg::Connected`]; an incoming publish is parsed and forwarded;
/// a connection error backs off (non-blocking, since this is its own task) and
/// lets the event loop reconnect. Returns when the worker's receiver is gone.
///
/// # Arguments
///
/// * `eventloop` - The MQTT event loop to drive.
/// * `client` - The MQTT client, for subscribing.
/// * `topics` - The bridge's topic layout.
/// * `tx` - The channel to the device worker.
/// * `export_limit_max` - The export-limit cap, for command validation.
#[expect(
    clippy::cognitive_complexity,
    reason = "a flat dispatch over the event loop's packet variants"
)]
async fn mqtt_driver(
    mut eventloop: rumqttc::EventLoop,
    client: AsyncClient,
    topics: BridgeTopics,
    tx: mpsc::Sender<WorkerMsg>,
    export_limit_max: f64,
) {
    let mut backoff = MIN_BACKOFF;
    let mut connected = false;
    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                backoff = MIN_BACKOFF;
                connected = true;
                info!("connected to MQTT broker; subscribing to command topics");
                for topic in [
                    &topics.panels_command,
                    &topics.export_limit_command,
                    &topics.grid_profile_command,
                ] {
                    if let Err(err) = client.subscribe(topic.clone(), QoS::AtLeastOnce).await {
                        warn!("subscribe to {topic} failed: {err}");
                    }
                }
                if tx.send(WorkerMsg::Connected).await.is_err() {
                    return;
                }
            }
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                if let Some(command) =
                    parse_command(&publish.topic, &publish.payload, &topics, export_limit_max)
                    && tx.send(WorkerMsg::Command(command)).await.is_err()
                {
                    return;
                }
            }
            Ok(_) => {}
            Err(err) => {
                // Tell the worker to stop publishing, once per disconnect: a full
                // client queue that `poll()` cannot drain while down would
                // otherwise wedge both tasks on the next reconnect.
                if connected {
                    connected = false;
                    if tx.send(WorkerMsg::Disconnected).await.is_err() {
                        return;
                    }
                }
                warn!("MQTT connection error: {err}; reconnecting in {backoff:?}");
                tokio::time::sleep(backoff).await;
                backoff = next_backoff(backoff);
            }
        }
    }
}

/// Publish the full connect-time sequence: availability, offline reset, poll.
///
/// Brings the bridge online, resets every entity to offline (clearing stale
/// retained `online`), clears the retired zero-export entity, performs the first
/// poll, then republishes discovery (with any freshly-read profile options) and
/// the cache state.
///
/// # Arguments
///
/// * `client` - The connected MQTT client.
/// * `controls` - The device controls.
/// * `topics` - The bridge's topic layout.
/// * `cache` - The state cache to seed from the first poll.
/// * `export_limit_max` - The export-limit `number`'s maximum, in watts.
async fn on_connect(
    client: &impl MqttPublisher,
    controls: &impl EnphaseControls,
    topics: &BridgeTopics,
    cache: &mut StateCache,
    export_limit_max: f64,
) {
    publish_availability(client, topics, "online").await;
    publish_all(client, offline_reset_messages(topics)).await;
    publish_all(
        client,
        vec![into_publish(zero_export_discovery_clear(NODE_ID))],
    )
    .await;
    poll_once(controls, cache).await;
    publish_discovery(client, topics, cache.profile_options(), export_limit_max).await;
    publish_all(client, cache.messages(topics)).await;
}

/// Poll the device once, publishing the refreshed state only when connected.
///
/// The poll always runs so the cache tracks the device, but publishing is gated on
/// the broker connection: while disconnected, `eventloop.poll()` cannot drain the
/// client's request queue, so publishing anyway would fill it and wedge the driver
/// and worker against each other on reconnect. The next
/// [`Connected`](WorkerMsg::Connected) republishes the full cache through
/// [`on_connect`].
///
/// # Arguments
///
/// * `connected` - Whether the broker connection is currently up.
/// * `client` - The publish seam.
/// * `controls` - The device controls to read.
/// * `topics` - The bridge's topic layout.
/// * `cache` - The state cache to refresh.
/// * `export_limit_max` - The export-limit `number`'s maximum, in watts.
async fn poll_and_publish(
    connected: bool,
    client: &impl MqttPublisher,
    controls: &impl EnphaseControls,
    topics: &BridgeTopics,
    cache: &mut StateCache,
    export_limit_max: f64,
) {
    poll_once(controls, cache).await;
    if connected {
        publish_discovery(client, topics, cache.profile_options(), export_limit_max).await;
        publish_all(client, cache.messages(topics)).await;
    }
}

/// Convert a discovery message into a retained [`PublishMessage`].
///
/// # Arguments
///
/// * `message` - The discovery message.
///
/// # Returns
///
/// The equivalent retained publish message.
fn into_publish(message: crate::discovery::DiscoveryMessage) -> PublishMessage {
    PublishMessage {
        topic: message.topic,
        payload: message.payload,
        retain: true,
    }
}

/// The device worker: own the controls and cache, service the channel and poll.
///
/// Runs on the `LocalSet` thread. Reacts to connection events and commands from
/// the driver, polls on the interval, and publishes the cache's view while the
/// broker is connected. On a shutdown signal it publishes availability `offline`
/// once and returns `Ok`, which ends `run_until` and drops the driver. If the
/// driver task ends first (its sender is dropped), the worker returns an error so
/// a supervisor restarts the process rather than leaving it silently idle.
///
/// # Arguments
///
/// * `client` - The connected MQTT client.
/// * `controls` - The device controls (owned; `!Send`).
/// * `topics` - The bridge's topic layout.
/// * `rx` - The channel from the MQTT driver.
/// * `config` - The resilience knobs (poll interval, thresholds, cap).
///
/// # Errors
///
/// Returns an error if the MQTT driver task ends before a shutdown signal.
#[expect(
    clippy::integer_division_remainder_used,
    clippy::cognitive_complexity,
    reason = "both live inside the tokio::select! macro expansion"
)]
async fn device_worker(
    client: impl MqttPublisher,
    controls: impl EnphaseControls,
    topics: BridgeTopics,
    mut rx: mpsc::Receiver<WorkerMsg>,
    config: ResilienceConfig,
) -> anyhow::Result<()> {
    let mut cache = StateCache::new(config.failure_threshold);
    let mut connected = false;
    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            () = &mut shutdown => {
                info!("shutdown signal received; marking bridge offline");
                // The authoritative offline signal is the Last Will: returning
                // here ends `run_until`, drops the driver, and closes the socket
                // without a DISCONNECT, so the broker fires the retained LWT. This
                // enqueued publish is a best-effort redundancy and may not flush
                // before the driver is dropped. Anything that later sends a
                // graceful DISCONNECT would suppress the LWT and must drain this
                // publish itself first.
                publish_availability(&client, &topics, "offline").await;
                return Ok(());
            }
            message = rx.recv() => {
                match message {
                    Some(WorkerMsg::Connected) => {
                        connected = true;
                        on_connect(&client, &controls, &topics, &mut cache, config.export_limit_max)
                            .await;
                    }
                    Some(WorkerMsg::Disconnected) => {
                        connected = false;
                    }
                    Some(WorkerMsg::Command(command)) => {
                        if let Err(err) =
                            apply_command(command, &controls, &mut cache, config.command_retries)
                                .await
                        {
                            warn!("command failed, reverting to last-known-good: {err:#}");
                        }
                        if connected {
                            publish_all(&client, cache.messages(&topics)).await;
                        }
                    }
                    None => {
                        return Err(anyhow::anyhow!(
                            "MQTT driver task ended before shutdown; restarting"
                        ));
                    }
                }
            }
            _ = interval.tick() => {
                poll_and_publish(
                    connected,
                    &client,
                    &controls,
                    &topics,
                    &mut cache,
                    config.export_limit_max,
                )
                .await;
            }
        }
    }
}

/// Run the bridge until a shutdown signal is received.
///
/// Connects to MQTT and runs two cooperating tasks on a `LocalSet`: a driver that
/// keeps the event loop polled, and a worker that owns the device controls and
/// cache. On SIGINT/SIGTERM the worker returns and the broker's Last Will marks
/// the bridge offline; a crash is covered by the same Last Will.
///
/// # Arguments
///
/// * `args` - The parsed CLI arguments.
///
/// # Errors
///
/// Returns an error if a required Enphase identity argument is missing (before the
/// loop starts), or if the MQTT driver task ends before shutdown. Transient
/// broker and Envoy failures are logged and folded into availability, not
/// returned.
#[inline]
pub async fn run(args: Args) -> anyhow::Result<()> {
    let topics = BridgeTopics::new(&args.topic_prefix);
    let controls = build_controls(&args)?;
    let options = build_mqtt_options(&args, &topics);

    let (client, eventloop) = AsyncClient::new(options, CLIENT_CAPACITY);
    let (tx, rx) = mpsc::channel::<WorkerMsg>(COMMAND_CAPACITY);

    let config = ResilienceConfig {
        poll_interval: args.poll_interval,
        failure_threshold: args.poll_failure_threshold,
        command_retries: args.command_retries,
        export_limit_max: args.export_limit_max,
    };

    info!("enphase-bridge starting; connecting to MQTT broker");

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            tokio::task::spawn_local(mqtt_driver(
                eventloop,
                client.clone(),
                topics.clone(),
                tx,
                config.export_limit_max,
            ));
            device_worker(client, controls, topics, rx, config).await
        })
        .await
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

    use crate::controls::{ExportLimit, GridProfiles, MockEnphaseControls};

    /// Mock controls that answer every read with a fixed, reachable value.
    fn reachable_controls() -> MockEnphaseControls {
        let mut controls = MockEnphaseControls::new();
        controls.expect_panels_state().returning(|| Ok(true));
        controls.expect_export_limit_state().returning(|| {
            Ok(ExportLimit {
                enabled: true,
                limit_w: 0.0,
            })
        });
        controls.expect_grid_profiles().returning(|| {
            Ok(GridProfiles {
                selected_id: "AUS_A:1.2".to_owned(),
                available: vec!["AUS_A:1.2".to_owned()],
            })
        });
        controls
    }

    #[tokio::test]
    async fn poll_and_publish_stays_silent_while_disconnected() {
        let controls = reachable_controls();
        let mut publisher = MockMqttPublisher::new();
        // The regression guard for the broker-outage deadlock: a disconnected tick
        // must not enqueue a single publish, or the queue fills and wedges both
        // tasks on reconnect.
        publisher.expect_publish().never();
        let topics = BridgeTopics::new("enphase");
        let mut cache = StateCache::new(3);
        poll_and_publish(false, &publisher, &controls, &topics, &mut cache, 5_000.0).await;
        // The poll still refreshed the cache, ready for the next reconnect.
        assert!(
            cache
                .messages(&topics)
                .iter()
                .any(|m| m.topic == topics.panels_state && m.payload == "ON")
        );
    }

    #[tokio::test]
    async fn poll_and_publish_publishes_while_connected() {
        let controls = reachable_controls();
        let mut publisher = MockMqttPublisher::new();
        publisher
            .expect_publish()
            .times(1..)
            .returning(|_topic, _retain, _payload| Ok(()));
        let topics = BridgeTopics::new("enphase");
        let mut cache = StateCache::new(3);
        poll_and_publish(true, &publisher, &controls, &topics, &mut cache, 5_000.0).await;
    }

    #[test]
    fn offline_reset_messages_cover_all_entities() {
        let topics = BridgeTopics::new("enphase");
        let msgs = offline_reset_messages(&topics);
        assert!(msgs.iter().all(|m| m.retain));
        let found: Vec<(&str, &str)> = msgs
            .iter()
            .map(|m| (m.topic.as_str(), m.payload.as_str()))
            .collect();
        assert_eq!(
            found,
            vec![
                (topics.reachability_state.as_str(), "OFF"),
                (topics.panels_availability.as_str(), "offline"),
                (topics.export_limit_availability.as_str(), "offline"),
                (topics.grid_profile_availability.as_str(), "offline"),
            ]
        );
    }
}
