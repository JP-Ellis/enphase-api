//! # Models for the Enphase API client
//!
//! This module contains data models used by the Enphase API client.

use serde::{Deserialize, Serialize};

/// Power state for an inverter or device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PowerState {
    /// Power is ON.
    On,
    /// Power is OFF.
    Off,
}

/// Response structure for getting the power status.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct PowerStatusResponse {
    /// Whether power is forced off.
    pub power_forced_off: bool,
}

/// Dynamic Power Export Limit (DPEL) settings for the Envoy.
///
/// Mirrors the `dynamic_pel_settings` object the Envoy exposes at
/// `/ivp/ss/dpel`. When an export limit is enforced the device closes a loop
/// on the consumption meter, capping how much power is fed back to the grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DpelSettings {
    /// Whether dynamic power export limiting is enabled.
    pub enable: bool,
    /// Whether an export limit is enforced.
    pub export_limit: bool,
    /// The export limit, in watts (`0.0` means full self-consumption).
    #[serde(rename = "limit_value_W")]
    pub limit_value_w: f64,
    /// The slew (ramp) rate the device applies when changing output, in watts
    /// per second.
    pub slew_rate: f64,
    /// Whether dynamic limiting (meter-driven adjustment) is enabled.
    pub enable_dynamic_limiting: bool,
}

/// Response body returned by `GET /ivp/ss/dpel`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
pub struct DpelResponse {
    /// The current dynamic power export limit settings.
    pub dynamic_pel_settings: DpelSettings,
}

/// A single AS/NZS 4777.2 grid profile available on the Envoy.
///
/// The device reports more per-profile metadata than is modelled here; only
/// the identifier needed to select a profile is captured, and unknown JSON
/// fields are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct GridProfile {
    /// The profile identifier, in `name:version` form (e.g. `"AUS_A:1.2"`).
    pub profile_id: String,
}

/// Response body returned by `GET /installer/agf/index.json`.
///
/// Lists the grid profile the Envoy is currently applying and the profiles it
/// has available to select.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct AgfIndexResponse {
    /// The name of the currently selected profile.
    pub selected_profile: String,
    /// The identifier of the currently selected profile (`name:version`).
    pub selected_profile_id: String,
    /// The profiles available to select.
    pub profiles: Vec<GridProfile>,
}

/// A single device entry within an [`InventorySection`].
///
/// The device reports more per-device metadata than is modelled here; only the
/// serial number is captured, and unknown JSON fields are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct InventoryDevice {
    /// The device's serial number.
    pub serial_num: String,
}

/// A section of the Envoy inventory returned by `GET /inventory.json`.
///
/// The inventory is an array of sections, each grouping devices of one type.
/// Inverters appear in the section whose `type` is `"PCU"` (power conditioning
/// unit).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct InventorySection {
    /// The section type (e.g. `"PCU"` for inverters).
    #[serde(rename = "type")]
    pub type_field: String,
    /// The devices in this section.
    pub devices: Vec<InventoryDevice>,
}

impl PowerState {
    /// Get the payload array value for this power state.
    pub(crate) fn payload_value(self) -> u8 {
        match self {
            PowerState::On => 0,
            PowerState::Off => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn power_state_on_payload() {
        let state = PowerState::On;
        assert_eq!(
            state.payload_value(),
            0,
            "PowerState::On should have payload value 0"
        );
    }

    #[test]
    fn power_state_off_payload() {
        let state = PowerState::Off;
        assert_eq!(
            state.payload_value(),
            1,
            "PowerState::Off should have payload value 1"
        );
    }

    #[test]
    fn deserialize_power_forced_off_true() {
        let json = r#"{"powerForcedOff": true}"#;
        let response: PowerStatusResponse =
            serde_json::from_str(json).expect("Should deserialize successfully");

        assert!(response.power_forced_off, "powerForcedOff should be true");
    }

    #[test]
    fn deserialize_power_forced_off_false() {
        let json = r#"{"powerForcedOff": false}"#;
        let response: PowerStatusResponse =
            serde_json::from_str(json).expect("Should deserialize successfully");

        assert!(!response.power_forced_off, "powerForcedOff should be false");
    }
}
