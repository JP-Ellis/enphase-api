//! Device state collection.
//!
//! Reads the device controls and assembles a
//! [`BridgeState`](crate::payload::BridgeState) for publishing.

use tracing::warn;

use crate::cache::{Entity, StateCache};
use crate::controls::{EnphaseControls, retry};
use crate::dispatch::handle_command;
use crate::payload::{BridgeState, Command};
use crate::reachability::{Reachability, classify};

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

/// Classify a read failure and update the cache accordingly.
///
/// A connection-level (unreachable) failure marks the whole cache unreachable; a
/// failure against a box that answered marks only the one entity as failed and
/// confirms reachability.
///
/// # Arguments
///
/// * `cache` - The cache to update.
/// * `entity` - The entity whose read failed.
/// * `error` - The error the read returned.
///
/// # Returns
///
/// `true` if the Envoy is unreachable and the caller should abort the poll.
fn handle_read_failure(cache: &mut StateCache, entity: Entity, error: &anyhow::Error) -> bool {
    match classify(error) {
        Reachability::Unreachable => {
            cache.mark_unreachable();
            true
        }
        Reachability::Reachable => {
            cache.mark_reachable();
            cache.record_failure(entity);
            false
        }
    }
}

/// Poll every entity once and fold the results into the cache.
///
/// Reads panels, export-limit, and grid-profiles in turn. A connection-level
/// failure on any read marks the whole cache unreachable and stops the poll; any
/// other outcome confirms reachability, recording a success or a per-entity
/// failure. Nothing is published here — the caller publishes `cache.messages()`.
///
/// # Arguments
///
/// * `controls` - The device controls to read.
/// * `cache` - The cache to update in place.
#[inline]
pub async fn poll_once(controls: &impl EnphaseControls, cache: &mut StateCache) {
    match controls.panels_state().await {
        Ok(on) => {
            cache.mark_reachable();
            cache.record_panels(on);
        }
        Err(error) => {
            if handle_read_failure(cache, Entity::Panels, &error) {
                return;
            }
        }
    }

    match controls.export_limit_state().await {
        Ok(limit) => {
            cache.mark_reachable();
            cache.record_export_limit(limit);
        }
        Err(error) => {
            if handle_read_failure(cache, Entity::ExportLimit, &error) {
                return;
            }
        }
    }

    match controls.grid_profiles().await {
        Ok(profiles) => {
            cache.mark_reachable();
            cache.record_grid_profile(profiles.selected_id, profiles.available);
        }
        Err(error) => if handle_read_failure(cache, Entity::GridProfile, &error) {},
    }
}

/// Apply a command, then reconcile the cache with the result.
///
/// The command is applied with retries. On success the affected entity is read
/// back: a successful read updates the cache; a failed read caches the
/// *commanded* value optimistically, because the command took effect and the
/// next poll will reconcile. If the apply itself fails after every retry, the
/// error is returned with the cache untouched, so the caller republishes the
/// last-known-good value and Home Assistant's optimistic toggle snaps back.
///
/// # Arguments
///
/// * `cmd` - The command to apply.
/// * `controls` - The device controls to drive.
/// * `cache` - The cache to update in place.
/// * `command_retries` - The number of attempts to apply the command.
///
/// # Errors
///
/// Returns the apply error when every attempt fails.
#[inline]
#[expect(
    clippy::cognitive_complexity,
    reason = "a flat per-command read-back reconciliation"
)]
pub async fn apply_command(
    cmd: Command,
    controls: &impl EnphaseControls,
    cache: &mut StateCache,
    command_retries: u32,
) -> anyhow::Result<()> {
    retry(command_retries, || handle_command(cmd.clone(), controls)).await?;

    match cmd {
        Command::SetPanels(on) => match controls.panels_state().await {
            Ok(read) => cache.record_panels(read),
            Err(err) => {
                warn!("panels read-back after command failed, caching commanded value: {err:#}");
                cache.record_panels(on);
            }
        },
        Command::SetExportLimit(watts) => match controls.export_limit_state().await {
            Ok(read) => cache.record_export_limit(read),
            Err(err) => {
                warn!(
                    "export-limit read-back after command failed, caching commanded value: {err:#}"
                );
                cache.record_export_limit(crate::controls::ExportLimit {
                    enabled: true,
                    limit_w: watts,
                });
            }
        },
        Command::SelectGridProfile(id) => match controls.grid_profiles().await {
            Ok(profiles) => cache.record_grid_profile(profiles.selected_id, profiles.available),
            Err(err) => {
                warn!(
                    "grid-profile read-back after command failed, caching commanded value: {err:#}"
                );
                let options = cache.profile_options().to_vec();
                cache.record_grid_profile(id, options);
            }
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::cache::StateCache;
    use crate::controls::{ExportLimit, GridProfiles, MockEnphaseControls};
    use crate::payload::{BridgeState, Command};
    use crate::topics::BridgeTopics;

    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn poll_once_marks_only_failed_entity_unavailable() {
        let mut mock = MockEnphaseControls::new();
        mock.expect_panels_state().returning(|| Ok(true));
        mock.expect_export_limit_state().returning(|| {
            Ok(ExportLimit {
                enabled: true,
                limit_w: 0.0,
            })
        });
        mock.expect_grid_profiles()
            .returning(|| Err(anyhow::anyhow!("index.json parse")));
        let mut cache = StateCache::new(1);
        poll_once(&mock, &mut cache).await;
        let topics = BridgeTopics::new("enphase");
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

    #[tokio::test]
    async fn apply_command_caches_commanded_value_when_readback_fails() {
        let mut mock = MockEnphaseControls::new();
        mock.expect_set_export_limit()
            .withf(|w| (*w - 5000.0_f64).abs() < f64::EPSILON)
            .returning(|_| Ok(()));
        mock.expect_export_limit_state()
            .returning(|| Err(anyhow::anyhow!("read-back blip")));
        let mut cache = StateCache::new(3);
        cache.mark_reachable();
        apply_command(Command::SetExportLimit(5000.0), &mock, &mut cache, 2)
            .await
            .expect("ok");
        let topics = BridgeTopics::new("enphase");
        let msgs = cache.messages(&topics);
        assert!(
            msgs.iter()
                .any(|m| m.topic == topics.export_limit_state && m.payload == "5000")
        );
    }

    #[tokio::test]
    async fn apply_command_failure_leaves_cache_for_revert() {
        let mut mock = MockEnphaseControls::new();
        mock.expect_set_export_limit()
            .times(2)
            .returning(|_| Err(anyhow::anyhow!("device refused")));
        let mut cache = StateCache::new(3);
        cache.mark_reachable();
        cache.record_export_limit(ExportLimit {
            enabled: true,
            limit_w: 0.0,
        });
        let result = apply_command(Command::SetExportLimit(5000.0), &mock, &mut cache, 2).await;
        let _err = result.expect_err("apply fails");
        let topics = BridgeTopics::new("enphase");
        let msgs = cache.messages(&topics);
        assert!(
            msgs.iter()
                .any(|m| m.topic == topics.export_limit_state && m.payload == "0")
        ); // last-known-good
    }

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
