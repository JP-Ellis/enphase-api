//! # Enphase Envoy Local Gateway Client
//!
//! This module provides a client for interacting with the Enphase Envoy local gateway.
//! The Envoy device is a gateway that provides local access to solar inverter data.
//!
//! ## Authentication
//!
//! The Envoy device typically uses JWT tokens for authentication. Tokens can be obtained
//! from the Enphase Entrez service. Some Envoy models may require username/password
//! authentication or digest authentication.
//!
//! ## Certificate Handling
//!
//! Envoy devices typically use self-signed certificates. This client is configured to
//! accept invalid certificates by default.

use alloc::sync::Arc;
use core::fmt::Display;
use std::path::PathBuf;

use crate::client::auth::{Credentials, load_cached_token, save_cached_token};
use crate::{
    client::{auth::AuthState, entrez::Entrez},
    error::Result,
    models::{
        AgfIndexResponse, DpelResponse, DpelSettings, InventorySection, PowerState,
        PowerStatusResponse,
    },
};
use serde::Serialize;
use tracing::{debug, instrument, warn};

/// Request body for `POST /ivp/ss/dpel`.
///
/// Wraps [`DpelSettings`] with the `filename`/`version` envelope the device
/// expects. Field order is significant: the Envoy is sent the settings exactly
/// as serialized here.
#[derive(Debug, Serialize)]
struct DpelRequest {
    /// The export limit settings to apply.
    dynamic_pel_settings: DpelSettings,
    /// Settings filename the device expects (always `"site_settings"`).
    filename: &'static str,
    /// Settings schema version the device expects.
    version: &'static str,
}

/// Map an HTTP response status to a [`Result`].
///
/// Returns `Ok(())` for any 2xx status, [`EnphaseError::Unauthorized`] for
/// HTTP 401 (so a caller holding credentials can re-authenticate and retry),
/// and [`EnphaseError::InvalidResponse`] for any other non-success status.
///
/// # Arguments
///
/// * `status` - The HTTP status returned by the device.
/// * `context` - A short description of the operation, used in the error
///   message (e.g. `"set power state"`).
fn check_status(status: reqwest::StatusCode, context: &str) -> Result<()> {
    if status.is_success() {
        return Ok(());
    }
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(crate::error::EnphaseError::Unauthorized(format!(
            "Failed to {context}: HTTP {status}"
        )));
    }
    Err(crate::error::EnphaseError::InvalidResponse(format!(
        "Failed to {context}: HTTP {status}"
    )))
}

/// Main client for the Enphase Envoy local gateway.
///
/// This client provides access to local solar production, consumption, and inverter data.
/// It handles session management and authentication with the Envoy device.
#[derive(Debug, Clone)]
pub struct Envoy {
    /// HTTP client for making requests.
    client: reqwest::Client,
    /// Base URL for the Envoy gateway.
    base_url: String,
    /// Optional auth source. When present, the client acquires and refreshes a
    /// token itself; when `None` it behaves as a plain, externally
    /// authenticated client. Shared behind an `Arc` so clones share one token.
    auth: Option<Arc<AuthState>>,
    /// Optional path for persisting the acquired token (mode `0600`). `None`
    /// keeps the token in memory only. Only meaningful alongside `auth`.
    token_cache: Option<PathBuf>,
}

impl Envoy {
    /// Create a new Envoy client with the given host.
    ///
    /// The host can be a hostname (e.g., "envoy.local") or IP address (e.g.,
    /// "192.168.1.100"). The client will connect via HTTPS by default.
    ///
    /// # Arguments
    ///
    /// * `host` - The hostname or IP address of the Envoy device
    ///
    /// # Returns
    ///
    /// Returns a new [`Envoy`] client configured for the given host.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Envoy;
    ///
    /// let client = Envoy::new("envoy.local");
    /// ```
    #[inline]
    #[expect(
        clippy::missing_panics_doc,
        clippy::expect_used,
        reason = "reqwest::Client::builder() with basic config cannot fail"
    )]
    pub fn new(host: impl Display) -> Self {
        let base_url = format!("https://{host}");

        let client = reqwest::Client::builder()
            .user_agent(format!("enphase-api/{}", env!("CARGO_PKG_VERSION")))
            .cookie_store(true)
            .timeout(core::time::Duration::from_secs(30))
            .danger_accept_invalid_certs(true)
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            base_url,
            auth: None,
            token_cache: None,
        }
    }

    /// Create a new Envoy client with the given host and HTTP client.
    ///
    /// This allows you to provide a custom `reqwest::Client` with specific
    /// configuration.
    ///
    /// Since the Envoy client uses self-signed certificates, ensure that the
    /// provided client is configured to accept them if necessary (or ignore
    /// certificate errors).
    ///
    /// Additionally, the Envoy client requires cookie storage to maintain
    /// session state, so ensure that the provided client has cookie store
    /// enabled.
    ///
    /// # Arguments
    ///
    /// * `host` - The hostname or IP address of the Envoy device
    /// * `client` - A configured `reqwest::Client`
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Envoy;
    ///
    /// let client = reqwest::Client::new();
    /// let envoy = Envoy::with_client("envoy.local", client);
    /// ```
    #[inline]
    pub fn with_client(host: impl Display, client: reqwest::Client) -> Self {
        let base_url = format!("https://{host}");

        Self {
            client,
            base_url,
            auth: None,
            token_cache: None,
        }
    }

    /// Create a self-authenticating Envoy client.
    ///
    /// Unlike [`new`](Self::new), the returned client owns its token lifecycle:
    /// on first use it logs in to Entrez, generates a JWT, and validates it
    /// against the device; when a request is rejected with
    /// [`Unauthorized`](crate::error::EnphaseError::Unauthorized) it refreshes
    /// the token and retries once. Callers invoke the control and read methods
    /// directly — no explicit [`authenticate`](Self::authenticate) call.
    ///
    /// The token is held in memory. To persist it across restarts, add
    /// [`with_token_cache`](Self::with_token_cache).
    ///
    /// # Arguments
    ///
    /// * `host` - The hostname or IP address of the Envoy device.
    /// * `entrez` - The Entrez client used to acquire tokens.
    /// * `credentials` - The credentials used to acquire tokens.
    ///
    /// # Returns
    ///
    /// Returns a new self-authenticating [`Envoy`] client.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::{Credentials, Entrez, Envoy};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let creds = Credentials::new("user@example.com", "password", "My Site", "121212121212");
    /// let envoy = Envoy::with_auth("envoy.local", Entrez::default(), creds);
    /// // No explicit authenticate; the client logs in on first use.
    /// let is_on = envoy.get_power_state("603980032").await?;
    /// # let _ = is_on;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[expect(
        clippy::missing_panics_doc,
        clippy::expect_used,
        reason = "reqwest::Client::builder() with basic config cannot fail"
    )]
    pub fn with_auth(host: impl Display, entrez: Entrez, credentials: Credentials) -> Self {
        let base_url = format!("https://{host}");

        let client = reqwest::Client::builder()
            .user_agent(format!("enphase-api/{}", env!("CARGO_PKG_VERSION")))
            .cookie_store(true)
            .timeout(core::time::Duration::from_secs(30))
            .danger_accept_invalid_certs(true)
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            base_url,
            auth: Some(Arc::new(AuthState::new(entrez, credentials))),
            token_cache: None,
        }
    }

    /// Persist the acquired token to a file (mode `0600`).
    ///
    /// By default a self-authenticating client holds its token in memory only,
    /// so every process restart triggers a fresh cloud login. With a cache
    /// path, the client reuses a previously stored token until it expires or is
    /// rejected, and writes each freshly acquired token back — minimising
    /// contact with Enphase. The file holds an installer-grade credential and is
    /// created `0600`.
    ///
    /// Caching is best-effort: if the token cannot be read or written (missing
    /// parent directory, read-only filesystem), the client logs a warning and
    /// carries on in memory rather than failing.
    ///
    /// Has no effect on a client built without [`with_auth`](Self::with_auth).
    ///
    /// # Arguments
    ///
    /// * `path` - The file the token is cached in.
    ///
    /// # Returns
    ///
    /// Returns the client with token caching enabled.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::{Credentials, Entrez, Envoy};
    ///
    /// let creds = Credentials::new("user@example.com", "password", "My Site", "121212121212");
    /// let envoy = Envoy::with_auth("envoy.local", Entrez::default(), creds)
    ///     .with_token_cache("/var/lib/enphase-bridge/token.jwt");
    /// ```
    #[inline]
    #[must_use]
    pub fn with_token_cache(mut self, path: impl Into<PathBuf>) -> Self {
        self.token_cache = Some(path.into());
        self
    }

    /// Ensure a token has been acquired, acquiring one if not.
    ///
    /// A no-op when no auth source is configured or a token is already held.
    /// Otherwise performs a full login → token → authenticate.
    async fn ensure_authenticated(&self) -> Result<()> {
        let Some(auth) = self.auth.as_ref() else {
            return Ok(());
        };
        if auth.token().is_some() {
            return Ok(());
        }

        // Try a cached token before a full login: if it still validates, reuse
        // it and skip the cloud round-trip. A missing, unreadable, or stale
        // cache falls through to a full login.
        if let Some(path) = self.token_cache.as_ref()
            && let Some(cached) = load_cached_token(path)
            && self.authenticate(&cached).await.is_ok()
        {
            debug!("Reusing cached token");
            auth.set_token(cached);
            return Ok(());
        }

        self.refresh().await
    }

    /// Force a fresh login and replace the held token.
    ///
    /// A no-op when no auth source is configured. Acquires a new JWT via Entrez,
    /// validates it against the device, stores it, and — when a cache path is
    /// configured — writes it to the cache file (mode `0600`).
    async fn refresh(&self) -> Result<()> {
        let Some(auth) = self.auth.as_ref() else {
            return Ok(());
        };
        let token = auth.acquire_token().await?;
        self.authenticate(&token).await?;
        // Persisting the token is best-effort: a write failure (missing parent
        // directory, read-only filesystem, full disk) must not fail an
        // authentication that otherwise succeeded, which would brick the client.
        // The error message carries no token value, so logging it is safe.
        if let Some(path) = self.token_cache.as_ref()
            && let Err(err) = save_cached_token(path, &token)
        {
            warn!(%err, "Failed to persist token cache; continuing in-memory");
        }
        auth.set_token(token);
        Ok(())
    }

    /// Run an operation, refreshing the token once on `Unauthorized`.
    ///
    /// With no auth source, runs `op` once and returns its result unchanged —
    /// so an externally authenticated client behaves exactly as before. With an
    /// auth source: ensures a token, runs `op`, and if it returns
    /// [`Unauthorized`](crate::error::EnphaseError::Unauthorized) refreshes the
    /// token and runs `op` exactly once more.
    async fn with_auth_retry<T>(&self, op: impl AsyncFn() -> Result<T>) -> Result<T> {
        if self.auth.is_none() {
            return op().await;
        }

        self.ensure_authenticated().await?;

        match op().await {
            Err(crate::error::EnphaseError::Unauthorized(_)) => {
                self.refresh().await?;
                op().await
            }
            other => other,
        }
    }

    /// Authenticate with the Envoy device using a JWT token.
    ///
    /// This validates that the provided token is valid by checking it against
    /// the Envoy device.
    ///
    /// # Arguments
    ///
    /// * `token` - The JWT token to authenticate with. This is typically
    ///   obtained from the Enphase Entrez service.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if authentication is successful.
    ///
    /// # Errors
    ///
    /// Returns an error if the token is invalid or the authentication check fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::{Envoy, Entrez};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let entrez = Entrez::default();
    /// entrez.login_with_env().await?;
    /// let token = entrez.generate_token("your-site-name", "your-envoy-serial-number", true).await?;
    /// let client = Envoy::new("envoy.local");
    /// client.authenticate(&token).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[instrument(skip(self, token), level = "debug")]
    pub async fn authenticate(&self, token: impl Display) -> Result<()> {
        debug!("Authenticating Envoy via JWT");

        let endpoint = format!("{}/auth/check_jwt", self.base_url);
        debug!("GET {endpoint}");

        let response = self
            .client
            .get(&endpoint)
            .bearer_auth(token.to_string())
            .send()
            .await?;

        let status = response.status();
        debug!("Status code: {}", status);

        let body = response.text().await?;

        if status == 200 && body.contains("Valid token") {
            debug!("JWT accepted");
            return Ok(());
        }

        Err(crate::error::EnphaseError::AuthenticationFailed(
            if body.is_empty() {
                "Invalid token or authentication failed".to_owned()
            } else {
                format!("JWT check failed: {}", body.trim())
            },
        ))
    }

    /// Set the power state of an inverter or device.
    ///
    /// This sends a command to the Envoy device to enable or disable power
    /// production on the specified device (identified by serial number).
    ///
    /// # Arguments
    ///
    /// * `serial` - The serial number of the device to control
    /// * `state` - The desired power state (`PowerState::On` or `PowerState::Off`)
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the power state change is successful.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the device does not respond
    /// correctly.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::{Envoy, models::PowerState};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Envoy::new("envoy.local");
    /// client.set_power_state("603980032", PowerState::Off).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[instrument(skip(self, serial, state), level = "debug")]
    pub async fn set_power_state(&self, serial: impl Display, state: PowerState) -> Result<()> {
        debug!(?state, "Setting power state");

        let serial_str = serial.to_string();

        self.with_auth_retry(async || {
            let endpoint = format!("{}/ivp/mod/{serial_str}/mode/power", self.base_url);
            debug!("PUT {endpoint}");

            // Build the JSON payload
            let payload = format!(r#"{{"length":1,"arr":[{}]}}"#, state.payload_value());

            let response = self
                .client
                .put(&endpoint)
                .header(
                    "Content-Type",
                    // This is not an error. Envoy expects the x-www-form-urlencoded
                    // content type, while the body is actually JSON.
                    "application/x-www-form-urlencoded; charset=UTF-8",
                )
                .body(payload)
                .send()
                .await?;

            let status = response.status();
            debug!("Status code: {}", status);

            // The endpoint returns 204 No Content on success (any 2xx is accepted).
            check_status(status, "set power state")?;
            debug!("Power state set successfully");
            Ok(())
        })
        .await
    }

    /// Get the power state of an inverter or device.
    ///
    /// This retrieves the current power state from the Envoy device for the
    /// specified device (identified by serial number).
    ///
    /// # Arguments
    ///
    /// * `serial` - The serial number of the device to query
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if power is on, `Ok(false)` if power is off.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Envoy;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Envoy::new("envoy.local");
    /// let is_on = client.get_power_state("603980032").await?;
    /// println!("Power is {}", if is_on { "on" } else { "off" });
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[instrument(skip(self, serial), level = "debug")]
    pub async fn get_power_state(&self, serial: impl Display) -> Result<bool> {
        debug!("Getting power state");

        let serial_str = serial.to_string();

        self.with_auth_retry(async || {
            let endpoint = format!("{}/ivp/mod/{serial_str}/mode/power", self.base_url);
            debug!("GET {endpoint}");

            let response = self
                .client
                .get(&endpoint)
                .header("Accept", "application/json, text/javascript, */*; q=0.01")
                .send()
                .await?;

            let status_code = response.status();
            debug!("Status code: {}", status_code);

            check_status(status_code, "get power state")?;

            let body = response.text().await?;
            debug!("Response body: {}", body);

            let status: PowerStatusResponse = serde_json::from_str(&body)?;
            debug!(?status, "Parsed power status");

            // powerForcedOff: true means power is OFF, so we invert it
            Ok(!status.power_forced_off)
        })
        .await
    }

    /// Enumerate the array's inverter serial numbers.
    ///
    /// Reads `/inventory.json` and returns the serial numbers of every device
    /// in the `PCU` (power conditioning unit) sections, in document order. The
    /// result drives whole-array controls that must act on each inverter.
    ///
    /// # Returns
    ///
    /// Returns the inverter serial numbers. An inventory with no `PCU` section
    /// yields an empty vector.
    ///
    /// # Errors
    ///
    /// Returns [`EnphaseError::Unauthorized`](crate::error::EnphaseError::Unauthorized)
    /// on HTTP 401, or another error if the request fails or the response
    /// cannot be parsed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Envoy;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Envoy::new("envoy.local");
    /// let serials = client.get_inverters().await?;
    /// println!("{} inverters", serials.len());
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[instrument(skip(self), level = "debug")]
    pub async fn get_inverters(&self) -> Result<Vec<String>> {
        debug!("Getting inverter inventory");

        self.with_auth_retry(async || {
            let endpoint = format!("{}/inventory.json", self.base_url);
            debug!("GET {endpoint}");

            let response = self
                .client
                .get(&endpoint)
                .header("Accept", "application/json, text/javascript, */*; q=0.01")
                .send()
                .await?;

            let status = response.status();
            debug!("Status code: {}", status);

            check_status(status, "get inventory")?;

            let body = response.text().await?;
            debug!("Response body: {}", body);

            let sections: Vec<InventorySection> = serde_json::from_str(&body)?;

            Ok(sections
                .into_iter()
                .filter(|section| section.type_field == "PCU")
                .flat_map(|section| section.devices)
                .map(|device| device.serial_num)
                .collect())
        })
        .await
    }

    /// Post a DPEL settings payload to the device.
    ///
    /// Shared by [`set_export_limit`](Self::set_export_limit) and
    /// [`disable_dpel`](Self::disable_dpel): both send the same
    /// `{dynamic_pel_settings, filename, version}` envelope and differ only in
    /// the settings they carry.
    async fn post_dpel(&self, settings: DpelSettings) -> Result<()> {
        self.with_auth_retry(async || {
            let endpoint = format!("{}/ivp/ss/dpel", self.base_url);
            debug!("POST {endpoint}");

            let body = serde_json::to_string(&DpelRequest {
                // Cloned so the request can be rebuilt if the auth retry fires.
                dynamic_pel_settings: settings.clone(),
                filename: "site_settings",
                version: "00.00.01",
            })?;

            let response = self
                .client
                .post(&endpoint)
                .header("Content-Type", "application/json")
                .body(body)
                .send()
                .await?;

            let status = response.status();
            debug!("Status code: {}", status);

            // The device's exact success status for this endpoint is unverified
            // (the spec documents the body, not the code); accept any 2xx.
            check_status(status, "set DPEL settings")?;
            debug!("DPEL settings accepted");
            Ok(())
        })
        .await
    }

    /// Set the grid export limit (DPEL / zero-export).
    ///
    /// Enables the dynamic power export limit and caps grid feed-in at
    /// `limit_w` watts. Pass `0.0` for full self-consumption (zero export).
    /// Requires an installer token and consumption current transformers (CTs);
    /// the device closes the loop on the consumption meter and the change takes
    /// effect in roughly a minute.
    ///
    /// # Arguments
    ///
    /// * `limit_w` - The maximum export power in watts (`0.0` = zero export).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the device accepts the new export limit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the device responds with a
    /// non-success status.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Envoy;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Envoy::new("envoy.local");
    /// client.set_export_limit(0.0).await?; // zero export
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[instrument(skip(self, limit_w), level = "debug")]
    pub async fn set_export_limit(&self, limit_w: f64) -> Result<()> {
        debug!(limit_w, "Setting export limit");

        self.post_dpel(DpelSettings {
            enable: true,
            export_limit: true,
            limit_value_w: limit_w,
            slew_rate: 100.0,
            enable_dynamic_limiting: false,
        })
        .await
    }

    /// Disable the grid export limit (DPEL).
    ///
    /// Turns off dynamic power export limiting, restoring unconstrained export
    /// (subject to the active grid profile). Requires an installer token.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the device accepts the change.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the device responds with a
    /// non-success status.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Envoy;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Envoy::new("envoy.local");
    /// client.disable_dpel().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[instrument(skip(self), level = "debug")]
    pub async fn disable_dpel(&self) -> Result<()> {
        debug!("Disabling DPEL");

        self.post_dpel(DpelSettings {
            enable: false,
            export_limit: false,
            limit_value_w: 0.0,
            slew_rate: 0.0,
            enable_dynamic_limiting: false,
        })
        .await
    }

    /// Get the current grid export limit (DPEL) settings.
    ///
    /// Reads the device's `dynamic_pel_settings` from `/ivp/ss/dpel`.
    ///
    /// # Returns
    ///
    /// Returns the current [`DpelSettings`].
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Envoy;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Envoy::new("envoy.local");
    /// let settings = client.get_dpel().await?;
    /// println!("export limit: {} W", settings.limit_value_w);
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[instrument(skip(self), level = "debug")]
    pub async fn get_dpel(&self) -> Result<DpelSettings> {
        debug!("Getting DPEL settings");

        self.with_auth_retry(async || {
            let endpoint = format!("{}/ivp/ss/dpel", self.base_url);
            debug!("GET {endpoint}");

            let response = self
                .client
                .get(&endpoint)
                .header("Accept", "application/json, text/javascript, */*; q=0.01")
                .send()
                .await?;

            let status = response.status();
            debug!("Status code: {}", status);

            check_status(status, "get DPEL settings")?;

            let body = response.text().await?;
            debug!("Response body: {}", body);

            let parsed: DpelResponse = serde_json::from_str(&body)?;
            Ok(parsed.dynamic_pel_settings)
        })
        .await
    }

    /// Get the grid profiles available on the Envoy.
    ///
    /// Reads `/installer/agf/index.json`, which reports the currently selected
    /// AS/NZS 4777.2 grid profile and the profiles available to select.
    ///
    /// # Returns
    ///
    /// Returns the [`AgfIndexResponse`] describing the selected and available
    /// profiles.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Envoy;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Envoy::new("envoy.local");
    /// let index = client.get_grid_profiles().await?;
    /// println!("active: {}", index.selected_profile_id);
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[instrument(skip(self), level = "debug")]
    pub async fn get_grid_profiles(&self) -> Result<AgfIndexResponse> {
        debug!("Getting grid profiles");

        self.with_auth_retry(async || {
            let endpoint = format!("{}/installer/agf/index.json", self.base_url);
            debug!("GET {endpoint}");

            let response = self
                .client
                .get(&endpoint)
                .header("Accept", "application/json, text/javascript, */*; q=0.01")
                .send()
                .await?;

            let status = response.status();
            debug!("Status code: {}", status);

            check_status(status, "get grid profiles")?;

            let body = response.text().await?;
            debug!("Response body: {}", body);

            let index: AgfIndexResponse = serde_json::from_str(&body)?;
            Ok(index)
        })
        .await
    }

    /// Select an AS/NZS 4777.2 grid profile by identifier.
    ///
    /// Sends the profile identifier (in `name:version` form, as listed by
    /// [`get_grid_profiles`](Self::get_grid_profiles)) to
    /// `/installer/agf/set_profile.json`. On 8.x firmware only one profile
    /// persists, so a profile must usually be uploaded with
    /// [`upload_grid_profile`](Self::upload_grid_profile) immediately before it
    /// can be selected.
    ///
    /// # Arguments
    ///
    /// * `profile_id` - The profile identifier, e.g. `"AUS_A:1.2"`.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the device accepts the profile selection.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the device responds with a
    /// non-success status.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Envoy;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Envoy::new("envoy.local");
    /// client.select_grid_profile("AUS_A:1.2").await?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[instrument(skip(self, profile_id), level = "debug")]
    pub async fn select_grid_profile(&self, profile_id: impl Display) -> Result<()> {
        debug!("Selecting grid profile");

        let profile_id_str = profile_id.to_string();

        self.with_auth_retry(async || {
            let endpoint = format!("{}/installer/agf/set_profile.json", self.base_url);
            debug!("PUT {endpoint}");

            // Serialize the body so the id is JSON-escaped. For a normal id (e.g.
            // "AUS_A:1.2") this is byte-identical to the literal form — the colon
            // round-trips unescaped — while a quote or backslash is escaped rather
            // than producing malformed JSON.
            let payload = serde_json::json!({ "selected_profile": &profile_id_str }).to_string();

            let response = self
                .client
                .put(&endpoint)
                .header(
                    "Content-Type",
                    // As with set_power_state, the Envoy expects this form content
                    // type while the body is actually JSON.
                    "application/x-www-form-urlencoded; charset=UTF-8",
                )
                .body(payload)
                .send()
                .await?;

            let status = response.status();
            debug!("Status code: {}", status);

            // The device's exact success status for this endpoint is unverified;
            // accept any 2xx (pinned against hardware in phase 2).
            check_status(status, "select grid profile")?;
            debug!("Grid profile selected");
            Ok(())
        })
        .await
    }

    /// Upload a grid-profile package to the Envoy.
    ///
    /// Sends the profile package bytes to
    /// `/installer/agf/upload_profile_package` as a multipart form with the
    /// single part named `file`. On 8.x firmware only one profile persists, so
    /// a package must usually be uploaded immediately before the matching
    /// profile is applied with
    /// [`select_grid_profile`](Self::select_grid_profile).
    ///
    /// # Arguments
    ///
    /// * `package` - The raw profile-package bytes to upload.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the device accepts the upload.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the device responds with a
    /// non-success status.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Envoy;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Envoy::new("envoy.local");
    /// let package = std::fs::read("AUS_A_1.2.json")?;
    /// client.upload_grid_profile(package).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[instrument(skip(self, package), level = "debug")]
    pub async fn upload_grid_profile(&self, package: Vec<u8>) -> Result<()> {
        debug!(bytes = package.len(), "Uploading grid profile package");

        self.with_auth_retry(async || {
            let endpoint = format!("{}/installer/agf/upload_profile_package", self.base_url);
            debug!("POST {endpoint}");

            // The device requires the part to be named `file`; a wrong name fails
            // silently on-device. The bytes are cloned so the multipart form can
            // be rebuilt if the auth retry fires.
            let form = reqwest::multipart::Form::new().part(
                "file",
                reqwest::multipart::Part::bytes(package.clone()).file_name("grid_profile.json"),
            );

            let response = self.client.post(&endpoint).multipart(form).send().await?;

            let status = response.status();
            debug!("Status code: {}", status);

            // The device's exact success status for this endpoint is unverified;
            // accept any 2xx (pinned against hardware in phase 2).
            check_status(status, "upload grid profile package")?;
            debug!("Grid profile package uploaded");
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::auth::{AuthState, Credentials};
    use crate::client::entrez::Entrez;
    use crate::models::PowerState;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{body_string, body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Load a fixture file as a raw JSON value.
    fn load_fixture(category: &str, name: &str) -> serde_json::Value {
        let fixture_path = format!("fixtures/{category}/{name}.json");
        let content = std::fs::read_to_string(&fixture_path)
            .unwrap_or_else(|_| panic!("Failed to read fixture: {fixture_path}"));
        serde_json::from_str(&content)
            .unwrap_or_else(|_| panic!("Failed to parse fixture: {fixture_path}"))
    }

    /// Build a `wiremock` response from a fixture's `status_code` and `body`.
    ///
    /// The fixtures store the captured device response; the tests mount only
    /// the status code and body, discarding the recorded headers.
    fn fixture_response(category: &str, name: &str) -> ResponseTemplate {
        let fixture = load_fixture(category, name);
        let status_code: u16 = fixture
            .get("status_code")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| v.try_into().ok())
            .expect("status_code is not a valid u16");
        let body = fixture
            .get("body")
            .and_then(serde_json::Value::as_str)
            .expect("body is not a string")
            .to_owned();
        ResponseTemplate::new(status_code).set_body_string(body)
    }

    /// Build an `Envoy` pointed at a mock server over plain HTTP.
    ///
    /// `wiremock` serves HTTP, so this bypasses the HTTPS/self-signed-cert
    /// setup `Envoy::new` applies and constructs the client against the mock
    /// server's URI directly.
    fn test_envoy(mock_server: &MockServer) -> Envoy {
        let test_client = reqwest::Client::builder()
            .cookie_store(true)
            .timeout(core::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build test client");
        Envoy {
            client: test_client,
            base_url: mock_server.uri(),
            auth: None,
            token_cache: None,
        }
    }

    #[tokio::test]
    async fn authenticate_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/auth/check_jwt"))
            .and(header("Authorization", "Bearer valid_token_here"))
            .respond_with(fixture_response("envoy", "authenticate-valid"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.authenticate("valid_token_here").await;

        assert!(
            result.is_ok(),
            "Authentication should succeed with valid token. Error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn authenticate_invalid_token() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/auth/check_jwt"))
            .respond_with(fixture_response("envoy", "authenticate-invalid"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.authenticate("invalid_token").await;

        assert!(result.is_err(), "Should fail with invalid token");
        if let Err(err) = result {
            assert!(
                matches!(err, crate::error::EnphaseError::AuthenticationFailed(_)),
                "Error should be AuthenticationFailed type"
            );
        }
    }

    #[tokio::test]
    async fn set_power_state() {
        let mock_server = MockServer::start().await;

        Mock::given(method("PUT"))
            .and(path("/ivp/mod/603980032/mode/power"))
            .and(header(
                "Content-Type",
                "application/x-www-form-urlencoded; charset=UTF-8",
            ))
            .and(body_string(r#"{"length":1,"arr":[0]}"#))
            .respond_with(fixture_response("envoy", "set-power-on"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.set_power_state("603980032", PowerState::On).await;

        assert!(result.is_ok(), "Setting power state to ON should succeed");
    }

    #[tokio::test]
    async fn get_power_state() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/ivp/mod/603980032/mode/power"))
            .respond_with(fixture_response("envoy", "get-power"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let is_on = client
            .get_power_state("603980032")
            .await
            .expect("Should succeed");

        assert!(is_on, "Power should be ON when powerForcedOff is false");
    }

    #[tokio::test]
    async fn get_power_state_invalid_json() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/ivp/mod/603980032/mode/power"))
            .respond_with(ResponseTemplate::new(200).set_body_string("Invalid JSON response"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.get_power_state("603980032").await;

        assert!(result.is_err(), "Should fail with invalid JSON");
        if let Err(err) = result {
            assert!(
                matches!(err, crate::error::EnphaseError::JsonError(_)),
                "Error should be JsonError type"
            );
        }
    }

    #[tokio::test]
    async fn set_export_limit_zero() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/ivp/ss/dpel"))
            .and(header("Content-Type", "application/json"))
            .and(body_string(
                r#"{"dynamic_pel_settings":{"enable":true,"export_limit":true,"limit_value_W":0.0,"slew_rate":100.0,"enable_dynamic_limiting":false},"filename":"site_settings","version":"00.00.01"}"#,
            ))
            .respond_with(fixture_response("envoy", "dpel-set"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.set_export_limit(0.0).await;

        assert!(
            result.is_ok(),
            "set_export_limit(0.0) should succeed. Error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn set_export_limit_5kw() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/ivp/ss/dpel"))
            .and(body_string(
                r#"{"dynamic_pel_settings":{"enable":true,"export_limit":true,"limit_value_W":5000.0,"slew_rate":100.0,"enable_dynamic_limiting":false},"filename":"site_settings","version":"00.00.01"}"#,
            ))
            .respond_with(fixture_response("envoy", "dpel-set"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.set_export_limit(5000.0).await;

        assert!(
            result.is_ok(),
            "set_export_limit(5000.0) should succeed. Error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn disable_dpel_sends_enable_false() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/ivp/ss/dpel"))
            .and(body_string_contains(r#""enable":false"#))
            .and(body_string_contains(r#""export_limit":false"#))
            .respond_with(fixture_response("envoy", "dpel-set"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.disable_dpel().await;

        assert!(
            result.is_ok(),
            "disable_dpel should succeed. Error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn get_dpel_parses() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/ivp/ss/dpel"))
            .respond_with(fixture_response("envoy", "dpel-get"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let settings = client.get_dpel().await.expect("get_dpel should succeed");

        assert!(
            settings.limit_value_w.abs() < f64::EPSILON,
            "limit_value_w should be 0.0, got {}",
            settings.limit_value_w
        );
        assert!(settings.export_limit, "export_limit should be true");
    }

    #[tokio::test]
    async fn set_export_limit_http_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/ivp/ss/dpel"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.set_export_limit(0.0).await;

        assert!(result.is_err(), "HTTP 401 should produce an error");
        if let Err(err) = result {
            assert!(
                matches!(err, crate::error::EnphaseError::Unauthorized(_)),
                "Error should be Unauthorized type"
            );
        }
    }

    #[tokio::test]
    async fn get_grid_profiles_parses() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/installer/agf/index.json"))
            .respond_with(fixture_response("envoy", "agf-index"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let index = client
            .get_grid_profiles()
            .await
            .expect("get_grid_profiles should succeed");

        assert_eq!(
            index.selected_profile_id, "AUS_A:1.2",
            "selected_profile_id should match the fixture"
        );
        assert_eq!(index.profiles.len(), 2, "there should be two profiles");
    }

    #[tokio::test]
    async fn select_grid_profile_body() {
        let mock_server = MockServer::start().await;

        Mock::given(method("PUT"))
            .and(path("/installer/agf/set_profile.json"))
            .and(header(
                "Content-Type",
                "application/x-www-form-urlencoded; charset=UTF-8",
            ))
            .and(body_string(r#"{"selected_profile":"AUS_A:1.2"}"#))
            .respond_with(fixture_response("envoy", "agf-set-profile"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.select_grid_profile("AUS_A:1.2").await;

        assert!(
            result.is_ok(),
            "select_grid_profile should succeed. Error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn select_grid_profile_http_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("PUT"))
            .and(path("/installer/agf/set_profile.json"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.select_grid_profile("AUS_A:1.2").await;

        assert!(result.is_err(), "HTTP 401 should produce an error");
        if let Err(err) = result {
            assert!(
                matches!(err, crate::error::EnphaseError::Unauthorized(_)),
                "Error should be Unauthorized type"
            );
        }
    }

    #[tokio::test]
    async fn select_grid_profile_escapes_special_chars() {
        let mock_server = MockServer::start().await;

        // An id containing a quote must be JSON-escaped, not interpolated raw;
        // a raw interpolation would emit malformed JSON.
        Mock::given(method("PUT"))
            .and(path("/installer/agf/set_profile.json"))
            .and(body_string(r#"{"selected_profile":"he\"llo"}"#))
            .respond_with(fixture_response("envoy", "agf-set-profile"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.select_grid_profile(r#"he"llo"#).await;

        assert!(
            result.is_ok(),
            "special characters in the profile id must be JSON-escaped. Error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn upload_grid_profile_sends_file_part() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/installer/agf/upload_profile_package"))
            // A wrong multipart field name silently fails on-device, so assert
            // the `file` part is present in the request body.
            .and(body_string_contains("name=\"file\""))
            .respond_with(fixture_response("envoy", "agf-upload"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.upload_grid_profile(b"PROFILE_BYTES".to_vec()).await;

        assert!(
            result.is_ok(),
            "upload_grid_profile should succeed. Error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn upload_grid_profile_http_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/installer/agf/upload_profile_package"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.upload_grid_profile(b"PROFILE_BYTES".to_vec()).await;

        assert!(result.is_err(), "HTTP 401 should produce an error");
        if let Err(err) = result {
            assert!(
                matches!(err, crate::error::EnphaseError::Unauthorized(_)),
                "Error should be Unauthorized type"
            );
        }
    }

    #[tokio::test]
    async fn get_power_state_unauthorized() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/ivp/mod/603980032/mode/power"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.get_power_state("603980032").await;

        assert!(result.is_err(), "HTTP 401 should produce an error");
        if let Err(err) = result {
            assert!(
                matches!(err, crate::error::EnphaseError::Unauthorized(_)),
                "Error should be Unauthorized type, got {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn get_dpel_unauthorized() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/ivp/ss/dpel"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.get_dpel().await;

        assert!(result.is_err(), "HTTP 401 should produce an error");
        if let Err(err) = result {
            assert!(
                matches!(err, crate::error::EnphaseError::Unauthorized(_)),
                "Error should be Unauthorized type, got {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn get_grid_profiles_unauthorized() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/installer/agf/index.json"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.get_grid_profiles().await;

        assert!(result.is_err(), "HTTP 401 should produce an error");
        if let Err(err) = result {
            assert!(
                matches!(err, crate::error::EnphaseError::Unauthorized(_)),
                "Error should be Unauthorized type, got {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn get_inverters_parses() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/inventory.json"))
            .respond_with(fixture_response("envoy", "inventory"))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let serials = client
            .get_inverters()
            .await
            .expect("get_inverters should succeed");

        assert_eq!(
            serials,
            vec!["482412345678".to_owned(), "482412345679".to_owned()],
            "get_inverters should return the PCU serials in document order"
        );
    }

    #[tokio::test]
    async fn get_inverters_empty_when_no_pcu() {
        let mock_server = MockServer::start().await;

        // An inventory with no PCU section (only a non-inverter section) must
        // yield an empty list, not an error.
        Mock::given(method("GET"))
            .and(path("/inventory.json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"[{"type":"NSRB","devices":[{"serial_num":"123456789012"}]}]"#,
                ),
            )
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let serials = client
            .get_inverters()
            .await
            .expect("get_inverters should succeed on a PCU-less inventory");

        assert!(
            serials.is_empty(),
            "an inventory with no PCU section should yield an empty list, got {serials:?}"
        );
    }

    #[tokio::test]
    async fn get_inverters_unauthorized() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/inventory.json"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&mock_server)
            .await;

        let client = test_envoy(&mock_server);

        let result = client.get_inverters().await;

        assert!(result.is_err(), "HTTP 401 should produce an error");
        if let Err(err) = result {
            assert!(
                matches!(err, crate::error::EnphaseError::Unauthorized(_)),
                "Error should be Unauthorized type, got {err:?}"
            );
        }
    }

    /// Build a plain-HTTP `reqwest::Client` (cookie store enabled) for mocks.
    fn test_http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .cookie_store(true)
            .timeout(core::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build test client")
    }

    /// Build an `Envoy` with an auth source pointed at mock servers.
    ///
    /// Mirrors [`test_envoy`] but attaches an [`AuthState`] so the auth state
    /// machine (login → token → authenticate, refresh-on-`Unauthorized`) is
    /// exercised over `wiremock`'s plain HTTP. `with_auth` itself prefixes
    /// `https://` and is covered by the ignored integration test.
    fn test_envoy_with_auth(
        envoy_server: &MockServer,
        entrez: Entrez,
        credentials: Credentials,
    ) -> Envoy {
        Envoy {
            client: test_http_client(),
            base_url: envoy_server.uri(),
            auth: Some(Arc::new(AuthState::new(entrez, credentials))),
            token_cache: None,
        }
    }

    /// Build an `Entrez` client pointed at a mock server over plain HTTP.
    fn test_entrez(entrez_server: &MockServer) -> Entrez {
        Entrez::with_client(entrez_server.uri(), test_http_client())
    }

    /// Build test [`Credentials`].
    fn test_credentials() -> Credentials {
        Credentials::new("user@example.com", "password", "My Site", "121212121212")
    }

    /// Mount the Entrez login + token endpoints on a mock server, each expecting
    /// exactly `times` hits (verified on server drop).
    async fn mount_entrez(entrez_server: &MockServer, times: u64) {
        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(ResponseTemplate::new(200).append_header("Set-Cookie", "s=1; Path=/"))
            .expect(times)
            .mount(entrez_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/entrez_tokens"))
            .respond_with(fixture_response("entrez", "generate-token-success"))
            .expect(times)
            .mount(entrez_server)
            .await;
    }

    #[tokio::test]
    async fn with_auth_refreshes_on_unauthorized() {
        let entrez_server = MockServer::start().await;
        let envoy_server = MockServer::start().await;

        // Entrez login + token are hit twice: once for the initial auth, once
        // for the refresh after the first 401.
        mount_entrez(&entrez_server, 2).await;

        // The Envoy accepts the JWT and, on the control call, returns 401 the
        // first time then 204.
        Mock::given(method("GET"))
            .and(path("/auth/check_jwt"))
            .respond_with(fixture_response("envoy", "authenticate-valid"))
            .mount(&envoy_server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/ivp/mod/603980032/mode/power"))
            .respond_with(ResponseTemplate::new(401))
            .up_to_n_times(1)
            .mount(&envoy_server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/ivp/mod/603980032/mode/power"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&envoy_server)
            .await;

        let envoy = test_envoy_with_auth(
            &envoy_server,
            test_entrez(&entrez_server),
            test_credentials(),
        );

        let result = envoy.set_power_state("603980032", PowerState::On).await;

        assert!(
            result.is_ok(),
            "set_power_state should succeed after a refresh. Error: {:?}",
            result.err()
        );
        // Entrez expectations (expect(2)) are verified when the servers drop.
    }

    #[tokio::test]
    async fn with_auth_still_unauthorized_returns_err() {
        let entrez_server = MockServer::start().await;
        let envoy_server = MockServer::start().await;

        // Refresh happens exactly once: initial auth + one refresh = 2 hits each.
        mount_entrez(&entrez_server, 2).await;

        Mock::given(method("GET"))
            .and(path("/auth/check_jwt"))
            .respond_with(fixture_response("envoy", "authenticate-valid"))
            .mount(&envoy_server)
            .await;
        // Always 401; expect exactly two attempts (initial + one retry).
        Mock::given(method("PUT"))
            .and(path("/ivp/mod/603980032/mode/power"))
            .respond_with(ResponseTemplate::new(401))
            .expect(2)
            .mount(&envoy_server)
            .await;

        let envoy = test_envoy_with_auth(
            &envoy_server,
            test_entrez(&entrez_server),
            test_credentials(),
        );

        let result = envoy.set_power_state("603980032", PowerState::On).await;

        assert!(result.is_err(), "a persistent 401 should return an error");
        if let Err(err) = result {
            assert!(
                matches!(err, crate::error::EnphaseError::Unauthorized(_)),
                "Error should be Unauthorized type, got {err:?}"
            );
        }
        // envoy.expect(2) and entrez.expect(2) are verified on server drop,
        // proving the retry happens exactly once.
    }

    #[tokio::test]
    async fn no_auth_envoy_surfaces_unauthorized() {
        let envoy_server = MockServer::start().await;

        // A no-auth Envoy must not retry: exactly one request, 401 surfaced.
        Mock::given(method("PUT"))
            .and(path("/ivp/mod/603980032/mode/power"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&envoy_server)
            .await;

        let envoy = test_envoy(&envoy_server);

        let result = envoy.set_power_state("603980032", PowerState::On).await;

        assert!(result.is_err(), "HTTP 401 should produce an error");
        if let Err(err) = result {
            assert!(
                matches!(err, crate::error::EnphaseError::Unauthorized(_)),
                "Error should be Unauthorized type, got {err:?}"
            );
        }
        // No Entrez server exists; a login attempt would fail the test.
        // envoy.expect(1) verifies no retry occurred.
    }

    /// Best-effort removal of a token-cache test file, ignoring any error.
    fn remove_cache(path: &std::path::Path) {
        match std::fs::remove_file(path) {
            Ok(()) | Err(_) => {}
        }
    }

    /// A unique path under the OS temp dir for a token-cache test.
    fn unique_cache_path(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "enphase-token-cache-{tag}-{}-{nanos}.jwt",
            std::process::id()
        ))
    }

    /// Mount the Envoy JWT check and a 204 power endpoint for an auth flow.
    async fn mount_envoy_auth_and_power(envoy_server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/auth/check_jwt"))
            .respond_with(fixture_response("envoy", "authenticate-valid"))
            .mount(envoy_server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/ivp/mod/603980032/mode/power"))
            .respond_with(ResponseTemplate::new(204))
            .mount(envoy_server)
            .await;
    }

    #[tokio::test]
    async fn token_cache_written_0600() {
        use std::os::unix::fs::PermissionsExt as _;

        let entrez_server = MockServer::start().await;
        let envoy_server = MockServer::start().await;
        let cache_path = unique_cache_path("0600");

        // No cache yet: one full login, which writes the cache.
        mount_entrez(&entrez_server, 1).await;
        mount_envoy_auth_and_power(&envoy_server).await;

        let envoy = test_envoy_with_auth(
            &envoy_server,
            test_entrez(&entrez_server),
            test_credentials(),
        )
        .with_token_cache(&cache_path);

        let result = envoy.set_power_state("603980032", PowerState::On).await;
        assert!(
            result.is_ok(),
            "auth flow should succeed. Error: {:?}",
            result.err()
        );

        let metadata = std::fs::metadata(&cache_path).expect("cache file should exist");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "cache file must be created mode 0600, got {mode:o}"
        );

        remove_cache(&cache_path);
    }

    #[tokio::test]
    async fn cached_token_skips_login() {
        let entrez_server = MockServer::start().await;
        let envoy_server = MockServer::start().await;
        let cache_path = unique_cache_path("skips");

        // A valid cached token: Entrez must never be contacted.
        std::fs::write(&cache_path, "SANITIZED_JWT_TOKEN").expect("write cache");
        mount_entrez(&entrez_server, 0).await;
        mount_envoy_auth_and_power(&envoy_server).await;

        let envoy = test_envoy_with_auth(
            &envoy_server,
            test_entrez(&entrez_server),
            test_credentials(),
        )
        .with_token_cache(&cache_path);

        let result = envoy.set_power_state("603980032", PowerState::On).await;
        assert!(
            result.is_ok(),
            "a valid cached token should authenticate without login. Error: {:?}",
            result.err()
        );

        remove_cache(&cache_path);
        // entrez.expect(0) is verified on server drop: no login/token hit.
    }

    #[tokio::test]
    async fn stale_cache_falls_back_to_login() {
        let entrez_server = MockServer::start().await;
        let envoy_server = MockServer::start().await;
        let cache_path = unique_cache_path("stale");

        // A stale cached token the device rejects: fall back to a full login.
        std::fs::write(&cache_path, "stale-junk").expect("write cache");
        mount_entrez(&entrez_server, 1).await;

        // The stale token is rejected; the freshly minted token is accepted.
        Mock::given(method("GET"))
            .and(path("/auth/check_jwt"))
            .and(header("Authorization", "Bearer stale-junk"))
            .respond_with(fixture_response("envoy", "authenticate-invalid"))
            .mount(&envoy_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/auth/check_jwt"))
            .respond_with(fixture_response("envoy", "authenticate-valid"))
            .mount(&envoy_server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/ivp/mod/603980032/mode/power"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&envoy_server)
            .await;

        let envoy = test_envoy_with_auth(
            &envoy_server,
            test_entrez(&entrez_server),
            test_credentials(),
        )
        .with_token_cache(&cache_path);

        let result = envoy.set_power_state("603980032", PowerState::On).await;
        assert!(
            result.is_ok(),
            "a stale cache should fall back to a full login. Error: {:?}",
            result.err()
        );

        remove_cache(&cache_path);
        // entrez.expect(1) verifies the fallback login happened.
    }

    #[tokio::test]
    async fn cache_write_failure_does_not_fail_auth() {
        let entrez_server = MockServer::start().await;
        let envoy_server = MockServer::start().await;

        // A cache path whose parent directory does not exist: the write fails,
        // but authentication itself succeeded, so the call must still succeed —
        // a bad cache path must not brick an otherwise-working client.
        let cache_path = unique_cache_path("badpath")
            .join("does-not-exist")
            .join("token.jwt");

        mount_entrez(&entrez_server, 1).await;
        mount_envoy_auth_and_power(&envoy_server).await;

        let envoy = test_envoy_with_auth(
            &envoy_server,
            test_entrez(&entrez_server),
            test_credentials(),
        )
        .with_token_cache(&cache_path);

        let result = envoy.set_power_state("603980032", PowerState::On).await;
        assert!(
            result.is_ok(),
            "a cache write failure must not fail authentication. Error: {:?}",
            result.err()
        );
    }

    #[test]
    fn credentials_debug_redacts_password() {
        let creds = Credentials::new(
            "user@example.com",
            "hunter2-secret-password",
            "My Site",
            "121212121212",
        );
        let rendered = format!("{creds:?}");
        assert!(
            !rendered.contains("hunter2-secret-password"),
            "Debug must not expose the password: {rendered}"
        );
    }

    #[test]
    fn auth_state_debug_redacts_token() {
        let creds = Credentials::new(
            "user@example.com",
            "hunter2-secret-password",
            "My Site",
            "121212121212",
        );
        let state = AuthState::new(Entrez::default(), creds);
        state.set_token("super-secret-jwt-value".to_owned());

        let rendered = format!("{state:?}");
        assert!(
            !rendered.contains("super-secret-jwt-value"),
            "Debug must not expose the token: {rendered}"
        );
        assert!(
            !rendered.contains("hunter2-secret-password"),
            "Debug must not expose the password via credentials: {rendered}"
        );
    }
}
