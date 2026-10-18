//! The Enphase control seam.
//!
//! The operations the bridge issues against the gateway sit behind the
//! [`EnphaseControls`] trait so command dispatch and state collection can be
//! unit-tested with a mock. [`EnvoyControls`] is the real implementation, a thin
//! wrapper over the library's self-authenticating [`enphase_api::Envoy`].

use core::future::Future;

use async_trait::async_trait;
use tracing::warn;

use enphase_api::Envoy;
use enphase_api::models::PowerState;

/// The export-limit (DPEL) state read back from the device.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ExportLimit {
    /// Whether export limiting is enabled.
    pub enabled: bool,
    /// The configured export limit, in watts.
    pub limit_w: f64,
}

/// The grid-profile state read back from the device.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct GridProfiles {
    /// The id of the currently selected profile.
    pub selected_id: String,
    /// The ids of the profiles available on the device.
    pub available: Vec<String>,
}

/// Decide whether the whole array counts as "on".
///
/// The array is "on" only when at least one inverter is present and every
/// inverter reports on.
///
/// # Arguments
///
/// * `states` - The per-inverter on/off states.
///
/// # Returns
///
/// `true` iff `states` is non-empty and all `true`.
#[inline]
#[must_use]
pub fn quorum_on(states: &[bool]) -> bool {
    !states.is_empty() && states.iter().all(|&state| state)
}

/// Run an async operation, retrying it up to `attempts` times.
///
/// Absorbs a transient failure (the documented "an inverter sometimes stays off,
/// retry" case) and surfaces the last error when every attempt fails. A zero
/// `attempts` still runs the operation once.
///
/// # Arguments
///
/// * `attempts` - The maximum number of attempts (treated as at least 1).
/// * `operation` - The operation to run; it may be called up to `attempts` times.
///
/// # Returns
///
/// The first success, or the error from the final attempt.
///
/// # Errors
///
/// Returns the last attempt's error when every attempt fails.
#[inline]
pub async fn retry<T, F, Fut>(attempts: u32, operation: F) -> anyhow::Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
{
    // The loop runs at least once and every failing attempt overwrites this
    // sentinel, so it is never returned; it exists only to avoid an `expect` on
    // the "zero attempts" path that `attempts.max(1)` already rules out.
    let mut last_error = anyhow::anyhow!("retry ran no attempts");
    for _ in 0..attempts.max(1) {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

/// The Enphase operations the bridge issues against the gateway.
///
/// Behind a trait so command dispatch and state collection can be driven by a
/// mock in tests.
#[cfg_attr(test, mockall::automock)]
#[async_trait(?Send)]
pub trait EnphaseControls {
    /// Turn the whole panel array on or off.
    ///
    /// # Arguments
    ///
    /// * `on` - `true` to turn panels on, `false` to turn them off.
    ///
    /// # Errors
    ///
    /// Returns an error if enumerating inverters or setting any inverter's
    /// power state fails.
    async fn set_panels(&self, on: bool) -> anyhow::Result<()>;

    /// Read whether the whole panel array is on.
    ///
    /// # Errors
    ///
    /// Returns an error if enumerating inverters or reading any inverter's
    /// power state fails.
    async fn panels_state(&self) -> anyhow::Result<bool>;

    /// Set the export limit, in watts.
    ///
    /// # Arguments
    ///
    /// * `watts` - The export limit to set.
    ///
    /// # Errors
    ///
    /// Returns an error if the device rejects the request.
    async fn set_export_limit(&self, watts: f64) -> anyhow::Result<()>;

    /// Disable export limiting (DPEL).
    ///
    /// # Errors
    ///
    /// Returns an error if the device rejects the request.
    async fn disable_export_limit(&self) -> anyhow::Result<()>;

    /// Read the current export-limit state.
    ///
    /// # Errors
    ///
    /// Returns an error if the device rejects the request or the response can
    /// not be parsed.
    async fn export_limit_state(&self) -> anyhow::Result<ExportLimit>;

    /// Read the available and selected grid profiles.
    ///
    /// # Errors
    ///
    /// Returns an error if the device rejects the request or the response can
    /// not be parsed.
    async fn grid_profiles(&self) -> anyhow::Result<GridProfiles>;

    /// Select the grid profile with the given id.
    ///
    /// # Arguments
    ///
    /// * `id` - The profile id to select.
    ///
    /// # Errors
    ///
    /// Returns an error if the device rejects the request.
    async fn select_grid_profile(&self, id: &str) -> anyhow::Result<()>;
}

/// The real [`EnphaseControls`], wrapping the library's self-authenticating
/// [`Envoy`]. It holds no token or credential state of its own.
#[derive(Debug)]
pub struct EnvoyControls {
    /// The self-authenticating Envoy client.
    envoy: Envoy,
}

impl EnvoyControls {
    /// Wrap an [`Envoy`] as an [`EnphaseControls`] implementation.
    ///
    /// # Arguments
    ///
    /// * `envoy` - A self-authenticating Envoy client.
    ///
    /// # Returns
    ///
    /// A new [`EnvoyControls`].
    #[inline]
    #[must_use]
    pub fn new(envoy: Envoy) -> Self {
        Self { envoy }
    }
}

#[async_trait(?Send)]
impl EnphaseControls for EnvoyControls {
    #[inline]
    async fn set_panels(&self, on: bool) -> anyhow::Result<()> {
        let state = if on { PowerState::On } else { PowerState::Off };
        let serials = self.envoy.get_inverters().await?;
        if serials.is_empty() {
            warn!("no inverters enumerated; panels command had no effect");
        }

        // Attempt every inverter, retrying each once here so a single flaky
        // inverter does not leave the rest of the array untouched; surface the
        // first persistent failure. The caller (`apply_command`) additionally
        // re-runs the whole call up to `--command-retries` times, so the two
        // layers compound: this one heals a lone inverter without redoing the
        // array, the outer one re-attempts the array as a unit.
        let mut first_error: Option<anyhow::Error> = None;
        for serial in serials {
            let outcome = retry(2, || async {
                self.envoy.set_power_state(&serial, state).await?;
                Ok(())
            })
            .await;
            if let Err(err) = outcome {
                warn!("failed to set inverter {serial}: {err:#}");
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
        }

        match first_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    #[inline]
    async fn panels_state(&self) -> anyhow::Result<bool> {
        let serials = self.envoy.get_inverters().await?;
        let mut states = Vec::with_capacity(serials.len());
        for serial in serials {
            states.push(self.envoy.get_power_state(serial).await?);
        }
        Ok(quorum_on(&states))
    }

    #[inline]
    async fn set_export_limit(&self, watts: f64) -> anyhow::Result<()> {
        self.envoy.set_export_limit(watts).await?;
        Ok(())
    }

    #[inline]
    async fn disable_export_limit(&self) -> anyhow::Result<()> {
        self.envoy.disable_dpel().await?;
        Ok(())
    }

    #[inline]
    async fn export_limit_state(&self) -> anyhow::Result<ExportLimit> {
        let dpel = self.envoy.get_dpel().await?;
        Ok(ExportLimit {
            enabled: dpel.enable && dpel.export_limit,
            limit_w: dpel.limit_value_w,
        })
    }

    #[inline]
    async fn grid_profiles(&self) -> anyhow::Result<GridProfiles> {
        let index = self.envoy.get_grid_profiles().await?;
        Ok(GridProfiles {
            selected_id: index.selected_profile_id,
            available: index
                .profiles
                .iter()
                .map(|profile| profile.profile_id.clone())
                .collect(),
        })
    }

    #[inline]
    async fn select_grid_profile(&self, id: &str) -> anyhow::Result<()> {
        self.envoy.select_grid_profile(id).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn quorum_on_all_true() {
        assert!(quorum_on(&[true, true]));
    }

    #[test]
    fn quorum_on_mixed_false() {
        assert!(!quorum_on(&[true, false]));
    }

    #[test]
    fn quorum_on_empty_false() {
        assert!(!quorum_on(&[]));
    }

    #[tokio::test]
    async fn retry_succeeds_on_a_later_attempt() {
        use core::sync::atomic::{AtomicU32, Ordering};

        let calls = AtomicU32::new(0);
        let result = retry(3, || async {
            if calls.fetch_add(1, Ordering::SeqCst) < 2 {
                anyhow::bail!("early attempt fails");
            }
            Ok(42_u32)
        })
        .await;
        assert_eq!(result.expect("retry succeeds"), 42);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_fails_after_all_attempts() {
        use core::sync::atomic::{AtomicU32, Ordering};

        let calls = AtomicU32::new(0);
        let result: anyhow::Result<u32> = retry(2, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("always fails")
        })
        .await;
        let _err = result.expect_err("all attempts fail");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retry_runs_at_least_once_for_zero_attempts() {
        use core::sync::atomic::{AtomicU32, Ordering};

        let calls = AtomicU32::new(0);
        let result = retry(0, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(7_u32)
        })
        .await;
        assert_eq!(result.expect("retry succeeds"), 7);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn mock_controls_usable() {
        let mut mock = MockEnphaseControls::new();
        mock.expect_panels_state().returning(|| Ok(true));
        let state = mock.panels_state().await.expect("panels_state succeeds");
        assert!(state);
    }
}
