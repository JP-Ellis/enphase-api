//! Home Assistant MQTT discovery configuration builders.
//!
//! These are pure functions: given the topic layout and the set of grid
//! profiles currently on the device, they produce the retained discovery
//! messages Home Assistant consumes to create the bridge's entities.

use serde_json::json;

use crate::topics::BridgeTopics;

/// A single retained MQTT discovery message: a config topic and its JSON
/// payload.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DiscoveryMessage {
    /// The `homeassistant/<component>/<node>/<object>/config` topic.
    pub topic: String,
    /// The serialised JSON discovery payload.
    pub payload: String,
}

/// Build the shared Home Assistant device block.
///
/// Every entity references the same device so Home Assistant groups them under
/// one "Enphase" device.
///
/// # Returns
///
/// The device JSON object shared by all four discovery payloads.
fn device_block() -> serde_json::Value {
    json!({
        "identifiers": ["enphase_bridge"],
        "name": "Enphase",
        "manufacturer": "JP-Ellis",
        "model": "IQ Gateway",
    })
}

/// Build the four Home Assistant discovery messages for the bridge entities.
///
/// The entities are, in order: panels switch, zero-export switch, export-limit
/// number, and grid-profile select.
///
/// # Arguments
///
/// * `topics` - The bridge's MQTT topic layout.
/// * `node_id` - The discovery node id embedded in each config topic.
/// * `profile_options` - The grid-profile ids offered by the `select` entity;
///   may be empty when the device reports no profiles.
///
/// # Returns
///
/// Exactly four [`DiscoveryMessage`]s, one per entity.
#[inline]
#[must_use]
pub fn discovery_configs(
    topics: &BridgeTopics,
    node_id: &str,
    profile_options: &[String],
) -> Vec<DiscoveryMessage> {
    let device = device_block();

    let panels = json!({
        "name": "Panels",
        "unique_id": "enphase_bridge_panels",
        "command_topic": topics.panels_command,
        "state_topic": topics.panels_state,
        "availability_topic": topics.availability,
        "payload_on": "ON",
        "payload_off": "OFF",
        "optimistic": false,
        "device": device.clone(),
    });

    let zero_export = json!({
        "name": "Zero export",
        "unique_id": "enphase_bridge_zero_export",
        "command_topic": topics.zero_export_command,
        "state_topic": topics.zero_export_state,
        "availability_topic": topics.availability,
        "payload_on": "ON",
        "payload_off": "OFF",
        "optimistic": false,
        "device": device.clone(),
    });

    let export_limit = json!({
        "name": "Export limit",
        "unique_id": "enphase_bridge_export_limit",
        "command_topic": topics.export_limit_command,
        "state_topic": topics.export_limit_state,
        "availability_topic": topics.availability,
        "min": 0_i64,
        "max": 20000_i64,
        "step": 100_i64,
        "unit_of_measurement": "W",
        "mode": "box",
        "device": device.clone(),
    });

    let grid_profile = json!({
        "name": "Grid profile",
        "unique_id": "enphase_bridge_grid_profile",
        "command_topic": topics.grid_profile_command,
        "state_topic": topics.grid_profile_state,
        "availability_topic": topics.availability,
        "options": profile_options,
        "device": device,
    });

    [
        ("switch", "panels", panels),
        ("switch", "zero_export", zero_export),
        ("number", "export_limit", export_limit),
        ("select", "grid_profile", grid_profile),
    ]
    .into_iter()
    .map(|(component, object, payload)| DiscoveryMessage {
        topic: format!("homeassistant/{component}/{node_id}/{object}/config"),
        payload: payload.to_string(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::topics::BridgeTopics;

    use pretty_assertions::assert_eq;

    #[test]
    fn discovery_has_four_entities() {
        let topics = BridgeTopics::new("enphase");
        let msgs = discovery_configs(&topics, "bridge", &["AUS_A:1.2".to_owned()]);
        assert_eq!(msgs.len(), 4);
        let expected = [
            "homeassistant/switch/bridge/panels/config",
            "homeassistant/switch/bridge/zero_export/config",
            "homeassistant/number/bridge/export_limit/config",
            "homeassistant/select/bridge/grid_profile/config",
        ];
        for (msg, want) in msgs.iter().zip(expected) {
            assert_eq!(msg.topic, want);
        }
    }

    #[test]
    fn discovery_fields_are_wired() {
        let topics = BridgeTopics::new("enphase");
        let msgs = discovery_configs(&topics, "bridge", &["AUS_A:1.2".to_owned()]);
        let panels = msgs.first().expect("panels message present");
        let value: serde_json::Value =
            serde_json::from_str(&panels.payload).expect("payload is valid JSON");
        assert_eq!(
            value
                .pointer("/unique_id")
                .and_then(serde_json::Value::as_str),
            Some("enphase_bridge_panels")
        );
        assert_eq!(
            value
                .pointer("/command_topic")
                .and_then(serde_json::Value::as_str),
            Some(topics.panels_command.as_str())
        );
        assert_eq!(
            value
                .pointer("/device/identifiers/0")
                .and_then(serde_json::Value::as_str),
            Some("enphase_bridge")
        );
    }

    #[test]
    fn discovery_select_options_empty_ok() {
        let topics = BridgeTopics::new("enphase");
        let msgs = discovery_configs(&topics, "bridge", &[]);
        let select = msgs.get(3).expect("select message present");
        let value: serde_json::Value =
            serde_json::from_str(&select.payload).expect("payload is valid JSON");
        assert_eq!(value.pointer("/options"), Some(&serde_json::json!([])));
    }
}
