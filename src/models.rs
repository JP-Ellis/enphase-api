//! # Models for the Enphase API client
//!
//! This module contains data models used by the Enphase API client.

/// Power state for an inverter or device
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PowerState {
    /// Power is ON
    On,
    /// Power is OFF
    Off,
}

impl PowerState {
    /// Get the payload array value for this power state
    pub(crate) fn payload_value(self) -> u8 {
        match self {
            PowerState::On => 0,
            PowerState::Off => 1,
        }
    }
}
