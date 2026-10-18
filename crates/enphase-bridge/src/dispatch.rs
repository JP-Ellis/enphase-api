//! MQTT command dispatch.
//!
//! Maps a parsed [`Command`](crate::payload::Command) to the matching
//! [`EnphaseControls`](crate::controls::EnphaseControls) call.

use crate::controls::EnphaseControls;
use crate::payload::Command;

/// Dispatch a command to the device controls.
///
/// Zero-export maps onto the export limit: turning it on sets a 0 W limit,
/// turning it off disables DPEL entirely.
///
/// # Arguments
///
/// * `cmd` - The command to execute.
/// * `controls` - The device controls to drive.
///
/// # Errors
///
/// Returns any error surfaced by the underlying control call.
#[inline]
pub async fn handle_command(cmd: Command, controls: &impl EnphaseControls) -> anyhow::Result<()> {
    match cmd {
        Command::SetPanels(on) => controls.set_panels(on).await,
        Command::SetZeroExport(true) => controls.set_export_limit(0.0).await,
        Command::SetZeroExport(false) => controls.disable_export_limit().await,
        Command::SetExportLimit(watts) => controls.set_export_limit(watts).await,
        Command::SelectGridProfile(id) => controls.select_grid_profile(&id).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::controls::MockEnphaseControls;
    use crate::payload::Command;

    #[tokio::test]
    async fn dispatch_set_panels() {
        let mut mock = MockEnphaseControls::new();
        mock.expect_set_panels()
            .withf(|on| *on)
            .times(1)
            .returning(|_| Ok(()));
        handle_command(Command::SetPanels(true), &mock)
            .await
            .expect("dispatch succeeds");
    }

    #[tokio::test]
    async fn dispatch_zero_export_on_sets_zero() {
        let mut mock = MockEnphaseControls::new();
        mock.expect_set_export_limit()
            .withf(|watts| *watts == 0.0_f64)
            .times(1)
            .returning(|_| Ok(()));
        handle_command(Command::SetZeroExport(true), &mock)
            .await
            .expect("dispatch succeeds");
    }

    #[tokio::test]
    async fn dispatch_zero_export_off_disables() {
        let mut mock = MockEnphaseControls::new();
        mock.expect_disable_export_limit()
            .times(1)
            .returning(|| Ok(()));
        handle_command(Command::SetZeroExport(false), &mock)
            .await
            .expect("dispatch succeeds");
    }

    #[tokio::test]
    async fn dispatch_select_profile() {
        let mut mock = MockEnphaseControls::new();
        mock.expect_select_grid_profile()
            .withf(|id| id == "AUS_A:1.2")
            .times(1)
            .returning(|_| Ok(()));
        handle_command(Command::SelectGridProfile("AUS_A:1.2".to_owned()), &mock)
            .await
            .expect("dispatch succeeds");
    }
}
