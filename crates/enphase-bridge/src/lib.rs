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
mod dispatch;
mod payload;
mod reachability;
mod state;
mod topics;

pub mod bridge;
pub mod cli;
pub mod logging;
pub mod version;

pub use controls::{EnphaseControls, EnvoyControls, ExportLimit, GridProfiles, quorum_on};
pub use discovery::{DiscoveryMessage, discovery_configs, zero_export_discovery_clear};
pub use dispatch::handle_command;
pub use payload::{BridgeState, Command, parse_command, state_messages};
pub use reachability::{Reachability, classify};
pub use state::collect_state;
pub use topics::BridgeTopics;
