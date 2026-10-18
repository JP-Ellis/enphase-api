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

use core::fmt::Display;

use crate::{
    error::Result,
    models::{AgfIndexResponse, DpelResponse, DpelSettings, PowerState, PowerStatusResponse},
};
use serde::Serialize;
use tracing::{debug, instrument};

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

        Self { client, base_url }
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

        Self { client, base_url }
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

        let endpoint = format!("{}/ivp/mod/{}/mode/power", self.base_url, serial);
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

        let endpoint = format!("{}/ivp/mod/{}/mode/power", self.base_url, serial);
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
    }

    /// Post a DPEL settings payload to the device.
    ///
    /// Shared by [`set_export_limit`](Self::set_export_limit) and
    /// [`disable_dpel`](Self::disable_dpel): both send the same
    /// `{dynamic_pel_settings, filename, version}` envelope and differ only in
    /// the settings they carry.
    async fn post_dpel(&self, settings: DpelSettings) -> Result<()> {
        let endpoint = format!("{}/ivp/ss/dpel", self.base_url);
        debug!("POST {endpoint}");

        let body = serde_json::to_string(&DpelRequest {
            dynamic_pel_settings: settings,
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

        let endpoint = format!("{}/installer/agf/set_profile.json", self.base_url);
        debug!("PUT {endpoint}");

        // Serialize the body so the id is JSON-escaped. For a normal id (e.g.
        // "AUS_A:1.2") this is byte-identical to the literal form — the colon
        // round-trips unescaped — while a quote or backslash is escaped rather
        // than producing malformed JSON.
        let payload = serde_json::json!({ "selected_profile": profile_id.to_string() }).to_string();

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

        let endpoint = format!("{}/installer/agf/upload_profile_package", self.base_url);
        debug!("POST {endpoint}");

        // The device requires the part to be named `file`; a wrong name fails
        // silently on-device.
        let form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(package).file_name("grid_profile.json"),
        );

        let response = self.client.post(&endpoint).multipart(form).send().await?;

        let status = response.status();
        debug!("Status code: {}", status);

        // The device's exact success status for this endpoint is unverified;
        // accept any 2xx (pinned against hardware in phase 2).
        check_status(status, "upload grid profile package")?;
        debug!("Grid profile package uploaded");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
