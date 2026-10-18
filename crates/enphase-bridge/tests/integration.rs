//! Integration tests for the bridge's Enphase control adapter.
//!
//! These require a live Envoy and valid credentials and are `#[ignore]`d by
//! default; they belong to phase-2 hardware testing.

#![cfg(test)]

use enphase_api::{Credentials, Entrez, Envoy};
use enphase_bridge::{
    BridgeTopics, EnphaseControls as _, EnvoyControls, Reachability, StateCache, classify,
    poll_once,
};

use pretty_assertions::assert_eq;

/// Exercise the real [`EnvoyControls`] against a live device.
///
/// Reads the Envoy host and Enphase credentials from the environment, builds a
/// self-authenticating client, and performs read-only control calls.
#[tokio::test]
#[ignore = "requires a live Envoy device and credentials (phase 2)"]
async fn envoy_controls_end_to_end() {
    let host = std::env::var("ENVOY_HOST").expect("ENVOY_HOST is set");
    let credentials = Credentials::new(
        std::env::var("ENPHASE_USERNAME").expect("ENPHASE_USERNAME is set"),
        std::env::var("ENPHASE_PASSWORD").expect("ENPHASE_PASSWORD is set"),
        std::env::var("ENPHASE_SITE_NAME").expect("ENPHASE_SITE_NAME is set"),
        std::env::var("ENPHASE_SERIAL_NUMBER").expect("ENPHASE_SERIAL_NUMBER is set"),
    );

    let controls = EnvoyControls::new(Envoy::with_auth(host, Entrez::default(), credentials));

    let panels = controls.panels_state().await.expect("read panels state");
    let export_limit = controls
        .export_limit_state()
        .await
        .expect("read export-limit state");
    let profiles = controls.grid_profiles().await.expect("read grid profiles");

    println!("panels_on={panels} export_limit={export_limit:?} profiles={profiles:?}");
}

/// A refused connection classifies as [`Reachability::Unreachable`].
///
/// Points an [`Envoy`] at a loopback port nothing listens on and confirms the
/// resulting error is read as a connection-level failure. The connect branch
/// cannot be unit-tested because `reqwest::Error` has no public constructor, so it
/// is exercised here against a refused `127.0.0.1:1` socket — deterministic and
/// network-free, so it runs in CI.
#[tokio::test]
async fn classify_connection_error_is_unreachable() {
    let envoy = Envoy::new("127.0.0.1:1");
    let err = envoy
        .get_inverters()
        .await
        .expect_err("connection to 127.0.0.1:1 is refused");
    assert_eq!(classify(&err.into()), Reachability::Unreachable);
}

/// An unreachable Envoy drives the whole cache offline through `poll_once`.
///
/// Points the real controls at a refused loopback socket and confirms `poll_once`
/// marks reachability off. This exercises the connection-level branch end to end,
/// which cannot be unit-tested because `reqwest::Error` has no public constructor;
/// the refused `127.0.0.1:1` socket is deterministic and network-free, so it runs
/// in CI.
#[tokio::test]
async fn poll_once_unreachable_drops_everything() {
    let controls = EnvoyControls::new(Envoy::new("127.0.0.1:1"));
    let mut cache = StateCache::new(1);
    cache.mark_reachable();
    poll_once(&controls, &mut cache).await;

    let topics = BridgeTopics::new("enphase");
    let msgs = cache.messages(&topics);
    let reachability = msgs
        .iter()
        .find(|m| m.topic == topics.reachability_state)
        .expect("reachability message present");
    assert_eq!(reachability.payload, "OFF");
    let panels = msgs
        .iter()
        .find(|m| m.topic == topics.panels_availability)
        .expect("panels availability present");
    assert_eq!(panels.payload, "offline");
}
