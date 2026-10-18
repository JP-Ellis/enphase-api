//! Home Assistant MQTT discovery configuration builders.
//!
//! These are pure functions: given the topic layout and the set of grid
//! profiles currently on the device, they produce the retained discovery
//! messages Home Assistant consumes to create the bridge's entities.

use serde_json::json;

use crate::topics::BridgeTopics;

/// The default export-limit `number` maximum, in watts: the owner's regulatory
/// cap. Also the CLI default for `--export-limit-max`.
pub(crate) const DEFAULT_EXPORT_LIMIT_MAX: f64 = 5_000.0;

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

/// Build the per-entity availability block for a control entity.
///
/// Each control entity lists two availability topics — the shared bridge-liveness
/// Last Will and its own per-entity topic — with `availability_mode: "all"`, so
/// it shows available only when the bridge is alive *and* the entity's own reads
/// are succeeding.
///
/// # Arguments
///
/// * `bridge` - The bridge-liveness (Last Will) availability topic.
/// * `entity` - The entity's own per-entity availability topic.
///
/// # Returns
///
/// The JSON availability array (bridge topic then entity topic) for the payload's
/// `availability` field; the caller sets `availability_mode` separately.
fn layered_availability(bridge: &str, entity: &str) -> serde_json::Value {
    json!([{ "topic": bridge }, { "topic": entity }])
}

/// Build the Home Assistant discovery messages for the bridge entities.
///
/// The entities are, in order: panels switch, export-limit number, grid-profile
/// select, and the reachability binary sensor. There is no zero-export entity;
/// [`zero_export_discovery_clear`] removes any that an earlier version announced.
///
/// # Arguments
///
/// * `topics` - The bridge's MQTT topic layout.
/// * `node_id` - The discovery node id embedded in each config topic.
/// * `profile_options` - The grid-profile ids offered by the `select` entity;
///   may be empty when the device reports no profiles.
/// * `export_limit_max` - The export-limit `number`'s maximum, in watts (the
///   owner's regulatory cap).
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
    export_limit_max: f64,
) -> Vec<DiscoveryMessage> {
    let device = device_block();

    let panels = json!({
        "name": "Panels",
        "unique_id": "enphase_bridge_panels",
        "command_topic": topics.panels_command,
        "state_topic": topics.panels_state,
        "availability": layered_availability(&topics.availability, &topics.panels_availability),
        "availability_mode": "all",
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
        "availability": layered_availability(&topics.availability, &topics.export_limit_availability),
        "availability_mode": "all",
        "min": 0_i64,
        "max": export_limit_max,
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
        "availability": layered_availability(&topics.availability, &topics.grid_profile_availability),
        "availability_mode": "all",
        "options": profile_options,
        "device": device.clone(),
    });

    let reachability = json!({
        "name": "Reachability",
        "unique_id": "enphase_bridge_reachability",
        "device_class": "connectivity",
        "state_topic": topics.reachability_state,
        "availability_topic": topics.availability,
        "payload_on": "ON",
        "payload_off": "OFF",
        "device": device,
    });

    [
        ("switch", "panels", panels),
        ("number", "export_limit", export_limit),
        ("select", "grid_profile", grid_profile),
        ("binary_sensor", "reachability", reachability),
    ]
    .into_iter()
    .map(|(component, object, payload)| DiscoveryMessage {
        topic: format!("homeassistant/{component}/{node_id}/{object}/config"),
        payload: payload.to_string(),
    })
    .collect()
}

/// Build the retained message that removes the retired zero-export entity.
///
/// An earlier bridge version announced a zero-export `switch`; publishing an
/// empty retained payload to its discovery config topic tells Home Assistant to
/// forget it after upgrade.
///
/// # Arguments
///
/// * `node_id` - The discovery node id embedded in the config topic.
///
/// # Returns
///
/// A [`DiscoveryMessage`] with the zero-export config topic and an empty payload.
#[inline]
#[must_use]
pub fn zero_export_discovery_clear(node_id: &str) -> DiscoveryMessage {
    DiscoveryMessage {
        topic: format!("homeassistant/switch/{node_id}/zero_export/config"),
        payload: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::topics::BridgeTopics;

    use pretty_assertions::assert_eq;

    #[test]
    fn discovery_has_four_entities_without_zero_export() {
        let topics = BridgeTopics::new("enphase");
        let msgs = discovery_configs(&topics, "bridge", &["AUS_A:1.2".to_owned()], 5000.0);
        let topics_seen: Vec<&str> = msgs.iter().map(|m| m.topic.as_str()).collect();
        assert_eq!(
            topics_seen,
            vec![
                "homeassistant/switch/bridge/panels/config",
                "homeassistant/number/bridge/export_limit/config",
                "homeassistant/select/bridge/grid_profile/config",
                "homeassistant/binary_sensor/bridge/reachability/config",
            ]
        );
    }

    #[test]
    fn control_entities_list_two_availability_topics_all_mode() {
        let topics = BridgeTopics::new("enphase");
        let msgs = discovery_configs(&topics, "bridge", &[], 5000.0);
        // Each control entity (index 0..3) layers the bridge LWT with its own
        // per-entity availability topic under `availability_mode: all`; a
        // wrongly wired export-limit or grid-profile entity must not slip through.
        let expected = [
            (0, "enphase/panels/availability"),
            (1, "enphase/export_limit/availability"),
            (2, "enphase/grid_profile/availability"),
        ];
        for (index, entity_topic) in expected {
            let entity: serde_json::Value =
                serde_json::from_str(&msgs.get(index).expect("control message").payload)
                    .expect("json");
            assert_eq!(
                entity
                    .pointer("/availability_mode")
                    .and_then(serde_json::Value::as_str),
                Some("all"),
                "entity at index {index} must use availability_mode all"
            );
            let topics_listed: Vec<&str> = entity
                .pointer("/availability")
                .and_then(serde_json::Value::as_array)
                .expect("availability array")
                .iter()
                .filter_map(|a| a.pointer("/topic").and_then(|t| t.as_str()))
                .collect();
            assert_eq!(
                topics_listed,
                vec!["enphase/availability", entity_topic],
                "entity at index {index} must layer the bridge LWT with its own topic"
            );
        }
    }

    #[test]
    fn export_limit_max_comes_from_argument() {
        let topics = BridgeTopics::new("enphase");
        let msgs = discovery_configs(&topics, "bridge", &[], 5000.0);
        let number: serde_json::Value =
            serde_json::from_str(&msgs.get(1).expect("export-limit message").payload)
                .expect("json");
        assert_eq!(
            number.pointer("/max").and_then(serde_json::Value::as_f64),
            Some(5_000.0_f64)
        );
    }

    #[test]
    fn reachability_is_connectivity_binary_sensor() {
        let topics = BridgeTopics::new("enphase");
        let msgs = discovery_configs(&topics, "bridge", &[], 5000.0);
        let rs: serde_json::Value =
            serde_json::from_str(&msgs.get(3).expect("reachability message").payload)
                .expect("json");
        assert_eq!(
            rs.pointer("/device_class")
                .and_then(serde_json::Value::as_str),
            Some("connectivity")
        );
        assert_eq!(
            rs.pointer("/state_topic")
                .and_then(serde_json::Value::as_str),
            Some("enphase/reachability/state")
        );
        // Reachability rides only the bridge LWT, so it stays readable when controls drop out.
        assert_eq!(
            rs.pointer("/availability_topic")
                .and_then(serde_json::Value::as_str),
            Some("enphase/availability")
        );
    }

    #[test]
    fn discovery_fields_are_wired() {
        let topics = BridgeTopics::new("enphase");
        let msgs = discovery_configs(&topics, "bridge", &["AUS_A:1.2".to_owned()], 5000.0);
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
        let msgs = discovery_configs(&topics, "bridge", &[], 5000.0);
        let select = msgs.get(2).expect("select message present");
        let value: serde_json::Value =
            serde_json::from_str(&select.payload).expect("payload is valid JSON");
        assert_eq!(value.pointer("/options"), Some(&serde_json::json!([])));
    }

    #[test]
    fn zero_export_clear_targets_old_topic() {
        let msg = zero_export_discovery_clear("bridge");
        assert_eq!(msg.topic, "homeassistant/switch/bridge/zero_export/config");
        assert_eq!(msg.payload, "");
    }
}
