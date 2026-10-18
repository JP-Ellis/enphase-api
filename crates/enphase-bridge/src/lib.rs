//! Enphase ↔ MQTT bridge library.
//!
//! A thin bridge that exposes an Enphase gateway's controls as Home Assistant
//! entities over MQTT discovery. The crate holds no pricing or decision logic:
//! Home Assistant owns those and drives the bridge through MQTT command topics.
//!
//! The modules split into pure data/transform helpers (topic layout, HA
//! discovery config building, and the command/state payload codec), the
//! device-facing control seam, and the async MQTT run loop. The binary is a
//! thin shim over this library.

#![expect(clippy::pub_use, reason = "Root API re-exports for convenience")]

mod controls;
mod discovery;
mod payload;
mod topics;

pub mod logging;
pub mod version;

pub use controls::{EnphaseControls, EnvoyControls, ExportLimit, GridProfiles, quorum_on};
pub use discovery::{DiscoveryMessage, discovery_configs};
pub use payload::{BridgeState, Command, parse_command, state_messages};
pub use topics::BridgeTopics;
