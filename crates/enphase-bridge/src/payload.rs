//! Command and state payload codec.
//!
//! Pure functions that translate between MQTT payloads and the bridge's typed
//! [`Command`] / [`BridgeState`] values.

use crate::topics::BridgeTopics;

/// A command received from Home Assistant over an MQTT command topic.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Command {
    /// Turn the whole panel array on (`true`) or off (`false`).
    SetPanels(bool),
    /// Enable (`true`) or disable (`false`) zero-export.
    SetZeroExport(bool),
    /// Set the export limit, in watts.
    SetExportLimit(f64),
    /// Select the grid profile with the given id.
    SelectGridProfile(String),
}

/// The bridge's view of the device state, for publishing to state topics.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct BridgeState {
    /// Whether the whole panel array is on.
    pub panels_on: bool,
    /// Whether zero-export (DPEL export limiting) is enabled.
    pub zero_export_on: bool,
    /// The current export limit, in watts.
    pub export_limit_w: f64,
    /// The id of the currently selected grid profile.
    pub selected_profile: String,
}

/// Parse an MQTT switch payload (`ON`/`OFF`, case-insensitive) into a bool.
///
/// # Arguments
///
/// * `payload` - The raw MQTT payload bytes.
///
/// # Returns
///
/// `Some(true)` for `ON`, `Some(false)` for `OFF`, `None` otherwise.
fn parse_switch(payload: &[u8]) -> Option<bool> {
    let text = core::str::from_utf8(payload).ok()?.trim();
    if text.eq_ignore_ascii_case("ON") {
        Some(true)
    } else if text.eq_ignore_ascii_case("OFF") {
        Some(false)
    } else {
        None
    }
}

/// Parse an incoming MQTT command.
///
/// Matches the topic against the four command topics and decodes the payload
/// accordingly.
///
/// # Arguments
///
/// * `topic` - The topic the message arrived on.
/// * `payload` - The raw MQTT payload bytes.
/// * `topics` - The bridge's topic layout.
///
/// # Returns
///
/// The decoded [`Command`], or `None` for an unknown topic or an unparsable
/// payload.
#[inline]
#[must_use]
pub fn parse_command(topic: &str, payload: &[u8], topics: &BridgeTopics) -> Option<Command> {
    if topic == topics.panels_command {
        parse_switch(payload).map(Command::SetPanels)
    } else if topic == topics.zero_export_command {
        parse_switch(payload).map(Command::SetZeroExport)
    } else if topic == topics.export_limit_command {
        let text = core::str::from_utf8(payload).ok()?.trim();
        text.parse::<f64>().ok().map(Command::SetExportLimit)
    } else if topic == topics.grid_profile_command {
        let text = core::str::from_utf8(payload).ok()?;
        Some(Command::SelectGridProfile(text.to_owned()))
    } else {
        None
    }
}

/// Format a watt value for a state topic.
///
/// Integral values print without a trailing decimal (e.g. `5000`), so Home
/// Assistant's number entity reads a clean value.
///
/// # Arguments
///
/// * `watts` - The export limit, in watts.
///
/// # Returns
///
/// The formatted string.
fn format_watts(watts: f64) -> String {
    if watts.fract() == 0.0 {
        format!("{watts:.0}")
    } else {
        format!("{watts}")
    }
}

/// Build the retained state messages for the current device state.
///
/// # Arguments
///
/// * `topics` - The bridge's topic layout.
/// * `state` - The device state to publish.
///
/// # Returns
///
/// `(topic, payload)` pairs for the four state topics, in entity order.
#[inline]
#[must_use]
pub fn state_messages(topics: &BridgeTopics, state: &BridgeState) -> Vec<(String, String)> {
    let on_off = |on: bool| if on { "ON" } else { "OFF" }.to_owned();
    vec![
        (topics.panels_state.clone(), on_off(state.panels_on)),
        (
            topics.zero_export_state.clone(),
            on_off(state.zero_export_on),
        ),
        (
            topics.export_limit_state.clone(),
            format_watts(state.export_limit_w),
        ),
        (
            topics.grid_profile_state.clone(),
            state.selected_profile.clone(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::topics::BridgeTopics;

    use pretty_assertions::assert_eq;

    #[test]
    fn parse_panels_on() {
        let topics = BridgeTopics::new("enphase");
        assert_eq!(
            parse_command(&topics.panels_command, b"ON", &topics),
            Some(Command::SetPanels(true))
        );
        assert_eq!(
            parse_command(&topics.panels_command, b"off", &topics),
            Some(Command::SetPanels(false))
        );
    }

    #[test]
    fn parse_export_limit_number() {
        let topics = BridgeTopics::new("enphase");
        assert_eq!(
            parse_command(&topics.export_limit_command, b"5000", &topics),
            Some(Command::SetExportLimit(5000.0))
        );
    }

    #[test]
    fn parse_select() {
        let topics = BridgeTopics::new("enphase");
        assert_eq!(
            parse_command(&topics.grid_profile_command, b"AUS_A:1.2", &topics),
            Some(Command::SelectGridProfile("AUS_A:1.2".to_owned()))
        );
    }

    #[test]
    fn parse_unknown_topic_is_none() {
        let topics = BridgeTopics::new("enphase");
        assert_eq!(parse_command("enphase/nope", b"ON", &topics), None);
    }

    #[test]
    fn parse_bad_number_is_none() {
        let topics = BridgeTopics::new("enphase");
        assert_eq!(
            parse_command(&topics.export_limit_command, b"not-a-number", &topics),
            None
        );
    }

    #[test]
    fn state_messages_formats() {
        let topics = BridgeTopics::new("enphase");
        let state = BridgeState {
            panels_on: true,
            zero_export_on: false,
            export_limit_w: 5000.0,
            selected_profile: "AUS_A:1.2".to_owned(),
        };
        let msgs = state_messages(&topics, &state);
        let expected = vec![
            (topics.panels_state.clone(), "ON".to_owned()),
            (topics.zero_export_state.clone(), "OFF".to_owned()),
            (topics.export_limit_state.clone(), "5000".to_owned()),
            (topics.grid_profile_state.clone(), "AUS_A:1.2".to_owned()),
        ];
        assert_eq!(msgs, expected);
    }
}
