//! MQTT command dispatch.
//!
//! Maps a parsed [`Command`](crate::payload::Command) to the matching
//! [`EnphaseControls`](crate::controls::EnphaseControls) call.

use crate::controls::EnphaseControls;
use crate::payload::Command;

/// Dispatch a command to the device controls.
///
/// The export-limit number drives DPEL directly: a 0 W limit is zero export, a
/// non-zero value caps export at that figure.
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
    async fn dispatch_set_export_limit() {
        let mut mock = MockEnphaseControls::new();
        mock.expect_set_export_limit()
            .withf(|watts| (*watts - 5000.0_f64).abs() < f64::EPSILON)
            .times(1)
            .returning(|_| Ok(()));
        handle_command(Command::SetExportLimit(5000.0), &mock)
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
