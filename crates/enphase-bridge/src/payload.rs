//! Command payload codec.
//!
//! Pure functions that translate MQTT command payloads into the bridge's typed
//! [`Command`] values. State publishing is produced by the
//! [`StateCache`](crate::cache::StateCache).

use crate::topics::BridgeTopics;

/// A command received from Home Assistant over an MQTT command topic.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Command {
    /// Turn the whole panel array on (`true`) or off (`false`).
    SetPanels(bool),
    /// Set the export limit, in watts.
    SetExportLimit(f64),
    /// Select the grid profile with the given id.
    SelectGridProfile(String),
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
/// * `export_limit_max` - The maximum export limit, in watts, to accept; mirrors
///   the `max` the `number` discovery config advertises.
///
/// # Returns
///
/// The decoded [`Command`], or `None` for an unknown topic or an unparsable
/// payload.
#[inline]
#[must_use]
pub fn parse_command(
    topic: &str,
    payload: &[u8],
    topics: &BridgeTopics,
    export_limit_max: f64,
) -> Option<Command> {
    if topic == topics.panels_command {
        parse_switch(payload).map(Command::SetPanels)
    } else if topic == topics.export_limit_command {
        let text = core::str::from_utf8(payload).ok()?.trim();
        let watts = text.parse::<f64>().ok()?;
        // Reject non-finite, negative, and out-of-range values (the range check
        // also excludes NaN and infinities), matching the bounds discovery
        // advertises, so a stray publisher cannot push a nonsense DPEL limit.
        (0.0..=export_limit_max)
            .contains(&watts)
            .then_some(Command::SetExportLimit(watts))
    } else if topic == topics.grid_profile_command {
        let text = core::str::from_utf8(payload).ok()?.trim();
        if text.is_empty() {
            return None;
        }
        Some(Command::SelectGridProfile(text.to_owned()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::topics::BridgeTopics;

    use pretty_assertions::assert_eq;

    /// A cap comfortably above every value these tests parse.
    const TEST_CAP: f64 = 20_000.0;

    #[test]
    fn parse_panels_on() {
        let topics = BridgeTopics::new("enphase");
        assert_eq!(
            parse_command(&topics.panels_command, b"ON", &topics, TEST_CAP),
            Some(Command::SetPanels(true))
        );
        assert_eq!(
            parse_command(&topics.panels_command, b"off", &topics, TEST_CAP),
            Some(Command::SetPanels(false))
        );
    }

    #[test]
    fn parse_export_limit_number() {
        let topics = BridgeTopics::new("enphase");
        assert_eq!(
            parse_command(&topics.export_limit_command, b"5000", &topics, TEST_CAP),
            Some(Command::SetExportLimit(5000.0))
        );
    }

    #[test]
    fn parse_select() {
        let topics = BridgeTopics::new("enphase");
        assert_eq!(
            parse_command(
                &topics.grid_profile_command,
                b"AUS_A:1.2",
                &topics,
                TEST_CAP
            ),
            Some(Command::SelectGridProfile("AUS_A:1.2".to_owned()))
        );
    }

    #[test]
    fn parse_unknown_topic_is_none() {
        let topics = BridgeTopics::new("enphase");
        assert_eq!(
            parse_command("enphase/nope", b"ON", &topics, TEST_CAP),
            None
        );
    }

    #[test]
    fn parse_bad_number_is_none() {
        let topics = BridgeTopics::new("enphase");
        assert_eq!(
            parse_command(
                &topics.export_limit_command,
                b"not-a-number",
                &topics,
                TEST_CAP
            ),
            None
        );
    }

    #[test]
    fn parse_export_limit_rejects_out_of_range_and_non_finite() {
        let topics = BridgeTopics::new("enphase");
        for payload in [
            b"-5".as_slice(),
            b"50000".as_slice(),
            b"nan".as_slice(),
            b"inf".as_slice(),
        ] {
            assert_eq!(
                parse_command(&topics.export_limit_command, payload, &topics, TEST_CAP),
                None,
                "payload {payload:?} should be rejected"
            );
        }
        // The boundary values are accepted.
        assert_eq!(
            parse_command(&topics.export_limit_command, b"0", &topics, TEST_CAP),
            Some(Command::SetExportLimit(0.0))
        );
        assert_eq!(
            parse_command(&topics.export_limit_command, b"20000", &topics, TEST_CAP),
            Some(Command::SetExportLimit(20000.0))
        );
    }

    #[test]
    fn parse_export_limit_rejects_above_cap() {
        let topics = BridgeTopics::new("enphase");
        // With a 5000 W cap, a 6000 W request is refused but the cap itself is accepted.
        assert_eq!(
            parse_command(&topics.export_limit_command, b"6000", &topics, 5_000.0),
            None
        );
        assert_eq!(
            parse_command(&topics.export_limit_command, b"5000", &topics, 5_000.0),
            Some(Command::SetExportLimit(5000.0))
        );
    }

    #[test]
    fn parse_select_trims_and_rejects_empty() {
        let topics = BridgeTopics::new("enphase");
        assert_eq!(
            parse_command(
                &topics.grid_profile_command,
                b"  AUS_A:1.2  ",
                &topics,
                TEST_CAP
            ),
            Some(Command::SelectGridProfile("AUS_A:1.2".to_owned()))
        );
        assert_eq!(
            parse_command(&topics.grid_profile_command, b"   ", &topics, TEST_CAP),
            None
        );
    }
}
