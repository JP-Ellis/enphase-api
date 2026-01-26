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
    models::{PowerState, PowerStatusResponse},
};
use tracing::{debug, instrument};

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

        // The endpoint returns 204 No Content on success
        if status == 204 {
            debug!("Power state set successfully");
            return Ok(());
        }

        Err(crate::error::EnphaseError::InvalidResponse(format!(
            "Failed to set power state: HTTP {status}"
        )))
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

        let body = response.text().await?;
        debug!("Response body: {}", body);

        let status: PowerStatusResponse = serde_json::from_str(&body)?;
        debug!(?status, "Parsed power status");

        // powerForcedOff: true means power is OFF, so we invert it
        Ok(!status.power_forced_off)
    }
}
