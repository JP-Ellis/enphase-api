//! The in-memory last-known-good device state.
//!
//! [`StateCache`] mirrors the device: each entity's last successful value, a
//! consecutive-failure counter, and its per-entity availability, plus the
//! connection-level reachable flag. It is the single source the bridge publishes
//! from, so a transient blip keeps the last-good value visible and only a
//! sustained failure flips an entity unavailable. The cache is never persisted —
//! a restart re-seeds from a fresh poll, never from retained MQTT.

use crate::controls::ExportLimit;
use crate::topics::BridgeTopics;

/// The payload Home Assistant's MQTT `number` reads as "unknown".
///
/// Published for the export-limit entity when DPEL is disabled or never read, so
/// Home Assistant shows unknown rather than a false cap. Confirm against HA's
/// current MQTT `number` schema (flagged for the Task-7 review hand-off).
const UNKNOWN_TOKEN: &str = "None";

/// A retained-or-not MQTT message the bridge should publish.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PublishMessage {
    /// The topic to publish to.
    pub topic: String,
    /// The payload to publish.
    pub payload: String,
    /// Whether the message is retained.
    pub retain: bool,
}

/// A control entity whose availability the cache tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Entity {
    /// The panels switch.
    Panels,
    /// The export-limit number.
    ExportLimit,
    /// The grid-profile select.
    GridProfile,
}

/// One entity's cached value, failure counter, and availability.
#[derive(Debug, Clone)]
struct EntityState<T> {
    /// The last successfully-read value, if any.
    value: Option<T>,
    /// Consecutive read failures since the last success.
    consecutive_failures: u32,
    /// Whether the entity is currently available.
    available: bool,
}

impl<T> EntityState<T> {
    /// A fresh, never-read entity: no value, not yet available.
    fn new() -> Self {
        Self {
            value: None,
            consecutive_failures: 0,
            available: false,
        }
    }

    /// Record a successful read: store the value, reset the counter, go available.
    fn record(&mut self, value: T) {
        self.value = Some(value);
        self.consecutive_failures = 0;
        self.available = true;
    }

    /// Record a failed read: bump the counter, go unavailable at the threshold.
    fn record_failure(&mut self, failure_threshold: u32) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.available = self.consecutive_failures < failure_threshold;
    }
}

/// The in-memory mirror of the device, with per-entity availability.
#[derive(Debug, Clone)]
pub struct StateCache {
    /// Consecutive failures before an entity is marked unavailable.
    failure_threshold: u32,
    /// Whether the Envoy was last reachable at the connection level.
    reachable: bool,
    /// The panels entity.
    panels: EntityState<bool>,
    /// The export-limit entity.
    export_limit: EntityState<ExportLimit>,
    /// The grid-profile entity (value is the selected profile id).
    grid_profile: EntityState<String>,
    /// The grid-profile ids available on the device, for discovery refresh.
    profile_options: Vec<String>,
}

impl StateCache {
    /// Build an empty cache.
    ///
    /// # Arguments
    ///
    /// * `failure_threshold` - Consecutive read failures before an entity is
    ///   marked unavailable.
    ///
    /// # Returns
    ///
    /// A cache with no values, every entity unavailable, and the Envoy assumed
    /// unreachable until the first successful contact.
    #[inline]
    #[must_use]
    pub fn new(failure_threshold: u32) -> Self {
        Self {
            failure_threshold,
            reachable: false,
            panels: EntityState::new(),
            export_limit: EntityState::new(),
            grid_profile: EntityState::new(),
            profile_options: Vec::new(),
        }
    }

    /// Record a successful panels read.
    ///
    /// # Arguments
    ///
    /// * `on` - Whether the panel array is on.
    #[inline]
    pub fn record_panels(&mut self, on: bool) {
        self.panels.record(on);
    }

    /// Record a successful export-limit read.
    ///
    /// # Arguments
    ///
    /// * `limit` - The export-limit state read from the device.
    #[inline]
    pub fn record_export_limit(&mut self, limit: ExportLimit) {
        self.export_limit.record(limit);
    }

    /// Record a successful grid-profile read.
    ///
    /// # Arguments
    ///
    /// * `selected` - The currently selected profile id.
    /// * `available` - The profile ids available on the device.
    #[inline]
    pub fn record_grid_profile(&mut self, selected: String, available: Vec<String>) {
        self.grid_profile.record(selected);
        self.profile_options = available;
    }

    /// Record a read failure for one entity.
    ///
    /// The entity stays available (holding its last value) until the configured
    /// threshold of consecutive failures is reached.
    ///
    /// # Arguments
    ///
    /// * `entity` - The entity whose read failed.
    #[inline]
    pub fn record_failure(&mut self, entity: Entity) {
        let threshold = self.failure_threshold;
        match entity {
            Entity::Panels => self.panels.record_failure(threshold),
            Entity::ExportLimit => self.export_limit.record_failure(threshold),
            Entity::GridProfile => self.grid_profile.record_failure(threshold),
        }
    }

    /// Mark the Envoy unreachable: reachability off, every entity unavailable.
    ///
    /// Cached values are left untouched so the last-known-good state is still
    /// available to republish once the box returns.
    #[inline]
    pub fn mark_unreachable(&mut self) {
        self.reachable = false;
        self.panels.available = false;
        self.export_limit.available = false;
        self.grid_profile.available = false;
    }

    /// Mark the Envoy reachable at the connection level.
    ///
    /// Per-entity availability is still governed by each entity's own successes
    /// and failures; this only lifts the connection-level flag.
    #[inline]
    pub fn mark_reachable(&mut self) {
        self.reachable = true;
    }

    /// The grid-profile ids available on the device.
    ///
    /// # Returns
    ///
    /// The profile ids, for refreshing the `select` entity's discovery options.
    #[inline]
    #[must_use]
    pub fn profile_options(&self) -> &[String] {
        &self.profile_options
    }

    /// Build the retained messages reflecting the current cached state.
    ///
    /// Emits the reachability state, each entity's availability, and each
    /// entity's state payload (the export-limit entity always publishes, as
    /// "unknown" when DPEL is disabled or never read).
    ///
    /// # Arguments
    ///
    /// * `topics` - The bridge's topic layout.
    ///
    /// # Returns
    ///
    /// The messages to publish, all retained.
    #[inline]
    #[must_use]
    pub fn messages(&self, topics: &BridgeTopics) -> Vec<PublishMessage> {
        let availability =
            |available: bool| if available { "online" } else { "offline" }.to_owned();
        let mut messages = vec![
            PublishMessage {
                topic: topics.reachability_state.clone(),
                payload: if self.reachable { "ON" } else { "OFF" }.to_owned(),
                retain: true,
            },
            PublishMessage {
                topic: topics.panels_availability.clone(),
                payload: availability(self.panels.available),
                retain: true,
            },
            PublishMessage {
                topic: topics.export_limit_availability.clone(),
                payload: availability(self.export_limit.available),
                retain: true,
            },
            PublishMessage {
                topic: topics.grid_profile_availability.clone(),
                payload: availability(self.grid_profile.available),
                retain: true,
            },
        ];

        if let Some(on) = self.panels.value {
            messages.push(PublishMessage {
                topic: topics.panels_state.clone(),
                payload: if on { "ON" } else { "OFF" }.to_owned(),
                retain: true,
            });
        }

        messages.push(PublishMessage {
            topic: topics.export_limit_state.clone(),
            payload: export_limit_payload(self.export_limit.value.as_ref()),
            retain: true,
        });

        if let Some(selected) = &self.grid_profile.value {
            messages.push(PublishMessage {
                topic: topics.grid_profile_state.clone(),
                payload: selected.clone(),
                retain: true,
            });
        }

        messages
    }
}

/// Render the export-limit state payload.
///
/// Enabled DPEL publishes the watt value (integral values trim the decimal);
/// disabled, never-read, or non-finite publishes the "unknown" token so Home
/// Assistant shows unknown rather than a false cap.
///
/// # Arguments
///
/// * `limit` - The cached export-limit value, if any.
///
/// # Returns
///
/// The payload string.
fn export_limit_payload(limit: Option<&ExportLimit>) -> String {
    match limit {
        Some(state) if state.enabled && state.limit_w.is_finite() => {
            if state.limit_w.fract() == 0.0 {
                format!("{:.0}", state.limit_w)
            } else {
                format!("{}", state.limit_w)
            }
        }
        _ => UNKNOWN_TOKEN.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_ne;

    #[test]
    fn first_success_makes_entity_available_with_value() {
        let topics = BridgeTopics::new("enphase");
        let mut cache = StateCache::new(3);
        cache.mark_reachable();
        cache.record_panels(true);
        let msgs = cache.messages(&topics);
        assert!(
            msgs.iter()
                .any(|m| m.topic == topics.panels_availability && m.payload == "online")
        );
        assert!(
            msgs.iter()
                .any(|m| m.topic == topics.panels_state && m.payload == "ON")
        );
    }

    #[test]
    fn transient_failure_under_threshold_keeps_last_value() {
        let topics = BridgeTopics::new("enphase");
        let mut cache = StateCache::new(3);
        cache.mark_reachable();
        cache.record_panels(true);
        cache.record_failure(Entity::Panels); // 1 of 3
        let msgs = cache.messages(&topics);
        assert!(
            msgs.iter()
                .any(|m| m.topic == topics.panels_availability && m.payload == "online")
        );
        assert!(
            msgs.iter()
                .any(|m| m.topic == topics.panels_state && m.payload == "ON")
        );
    }

    #[test]
    fn sustained_failure_marks_only_that_entity_unavailable() {
        let topics = BridgeTopics::new("enphase");
        let mut cache = StateCache::new(2);
        cache.mark_reachable();
        cache.record_panels(true);
        cache.record_grid_profile("AUS_A:1.2".to_owned(), vec![]);
        cache.record_failure(Entity::GridProfile);
        cache.record_failure(Entity::GridProfile); // reaches threshold
        let msgs = cache.messages(&topics);
        assert!(
            msgs.iter()
                .any(|m| m.topic == topics.grid_profile_availability && m.payload == "offline")
        );
        assert!(
            msgs.iter()
                .any(|m| m.topic == topics.panels_availability && m.payload == "online")
        );
    }

    #[test]
    fn mark_unreachable_drops_all_entities_and_reachability() {
        let topics = BridgeTopics::new("enphase");
        let mut cache = StateCache::new(3);
        cache.mark_reachable();
        cache.record_panels(true);
        cache.mark_unreachable();
        let msgs = cache.messages(&topics);
        assert!(
            msgs.iter()
                .any(|m| m.topic == topics.reachability_state && m.payload == "OFF")
        );
        assert!(
            msgs.iter()
                .any(|m| m.topic == topics.panels_availability && m.payload == "offline")
        );
    }

    #[test]
    fn export_limit_unknown_when_dpel_disabled() {
        let topics = BridgeTopics::new("enphase");
        let mut cache = StateCache::new(3);
        cache.mark_reachable();
        cache.record_export_limit(ExportLimit {
            enabled: false,
            limit_w: 5000.0,
        });
        let msgs = cache.messages(&topics);
        let state = msgs
            .iter()
            .find(|m| m.topic == topics.export_limit_state)
            .expect("export state");
        assert_ne!(state.payload, "5000"); // must not publish the stale cap as if enforced
    }
}
