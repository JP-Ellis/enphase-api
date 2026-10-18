//! MQTT topic layout for the bridge.
//!
//! Every topic is derived from a single configurable prefix, so the whole
//! namespace moves together when the prefix changes.

/// The set of MQTT topics the bridge publishes to and subscribes from.
///
/// Each field is `format!("{prefix}/<suffix>")` for a fixed suffix, built once
/// in [`BridgeTopics::new`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BridgeTopics {
    /// Availability topic carrying `online`/`offline` (also the Last Will).
    pub availability: String,
    /// State topic for the panels switch.
    pub panels_state: String,
    /// Command topic for the panels switch.
    pub panels_command: String,
    /// State topic for the export-limit number.
    pub export_limit_state: String,
    /// Command topic for the export-limit number.
    pub export_limit_command: String,
    /// State topic for the grid-profile select.
    pub grid_profile_state: String,
    /// Command topic for the grid-profile select.
    pub grid_profile_command: String,
    /// Per-entity availability topic for the panels switch.
    pub panels_availability: String,
    /// Per-entity availability topic for the export-limit number.
    pub export_limit_availability: String,
    /// Per-entity availability topic for the grid-profile select.
    pub grid_profile_availability: String,
    /// State topic for the connection-level reachability sensor.
    pub reachability_state: String,
}

impl BridgeTopics {
    /// Build the topic set for a given prefix.
    ///
    /// # Arguments
    ///
    /// * `prefix` - The MQTT topic prefix all bridge topics hang off.
    ///
    /// # Returns
    ///
    /// A [`BridgeTopics`] with every topic string precomputed.
    #[inline]
    #[must_use]
    pub fn new(prefix: &str) -> Self {
        Self {
            availability: format!("{prefix}/availability"),
            panels_state: format!("{prefix}/panels/state"),
            panels_command: format!("{prefix}/panels/command"),
            export_limit_state: format!("{prefix}/export_limit/state"),
            export_limit_command: format!("{prefix}/export_limit/command"),
            grid_profile_state: format!("{prefix}/grid_profile/state"),
            grid_profile_command: format!("{prefix}/grid_profile/command"),
            panels_availability: format!("{prefix}/panels/availability"),
            export_limit_availability: format!("{prefix}/export_limit/availability"),
            grid_profile_availability: format!("{prefix}/grid_profile/availability"),
            reachability_state: format!("{prefix}/reachability/state"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn topics_use_prefix() {
        let topics = BridgeTopics::new("enphase");
        assert_eq!(topics.panels_command, "enphase/panels/command");
        assert_eq!(topics.availability, "enphase/availability");
        assert_eq!(topics.export_limit_state, "enphase/export_limit/state");
        assert_eq!(topics.grid_profile_command, "enphase/grid_profile/command");
        assert_eq!(topics.panels_availability, "enphase/panels/availability");
        assert_eq!(
            topics.export_limit_availability,
            "enphase/export_limit/availability"
        );
        assert_eq!(
            topics.grid_profile_availability,
            "enphase/grid_profile/availability"
        );
        assert_eq!(topics.reachability_state, "enphase/reachability/state");
    }
}
