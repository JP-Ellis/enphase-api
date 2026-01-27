//! # Models for the Enphase API client
//!
//! This module contains data models used by the Enphase API client.

use serde::Deserialize;

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
