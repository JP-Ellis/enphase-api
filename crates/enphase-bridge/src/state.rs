//! Device state collection.
//!
//! Reads the device controls and assembles a
//! [`BridgeState`](crate::payload::BridgeState) for publishing.

use crate::controls::EnphaseControls;
use crate::payload::BridgeState;

/// Collect the current device state for publishing.
///
/// The zero-export switch reports on whenever DPEL export limiting is enabled,
/// regardless of the watt value; the export-limit number carries the actual
/// limit.
///
/// # Arguments
///
/// * `controls` - The device controls to read from.
///
/// # Errors
///
/// Returns any error surfaced by reading panel, export-limit, or grid-profile
/// state.
#[inline]
pub async fn collect_state(controls: &impl EnphaseControls) -> anyhow::Result<BridgeState> {
    let panels_on = controls.panels_state().await?;
    let export_limit = controls.export_limit_state().await?;
    let grid_profiles = controls.grid_profiles().await?;

    Ok(BridgeState {
        panels_on,
        zero_export_on: export_limit.enabled,
        export_limit_w: export_limit.limit_w,
        selected_profile: grid_profiles.selected_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::controls::{ExportLimit, GridProfiles, MockEnphaseControls};
    use crate::payload::BridgeState;

    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn collect_state_maps_controls() {
        let mut mock = MockEnphaseControls::new();
        mock.expect_panels_state().returning(|| Ok(true));
        mock.expect_export_limit_state().returning(|| {
            Ok(ExportLimit {
                enabled: true,
                limit_w: 0.0,
            })
        });
        mock.expect_grid_profiles().returning(|| {
            Ok(GridProfiles {
                selected_id: "AUS_A:1.2".to_owned(),
                available: vec![],
            })
        });

        let state = collect_state(&mock).await.expect("collect_state succeeds");

        assert_eq!(
            state,
            BridgeState {
                panels_on: true,
                zero_export_on: true,
                export_limit_w: 0.0,
                selected_profile: "AUS_A:1.2".to_owned(),
            }
        );
    }
}
