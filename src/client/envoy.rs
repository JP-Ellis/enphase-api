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

/// Main client for the Enphase Envoy local gateway
///
/// This client provides access to local solar production, consumption, and inverter data.
/// It handles session management and authentication with the Envoy device.
#[derive(Debug)]
pub struct Envoy {
    /// HTTP agent for making requests
    agent: ureq::Agent,
    /// Base URL for the Envoy gateway
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
    pub fn new(host: impl Display) -> Self {
        let base_url = format!("https://{host}");

        let agent = ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .disable_verification(true)
                    .build(),
            )
            .build()
            .new_agent();

        Self { agent, base_url }
    }

    /// Create a new Envoy client with the given host and agent.
    ///
    /// This allows you to provide a custom `ureq::Agent` with specific
    /// configuration.
    ///
    /// Since the Envoy client uses self-signed certificates, ensure that the
    /// provided agent is configured to accept them if necessary.
    ///
    /// # Arguments
    ///
    /// * `host` - The hostname or IP address of the Envoy device
    /// * `agent` - A configured `ureq::Agent`
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Envoy;
    ///
    /// let agent = ureq::agent();
    /// let client = Envoy::with_agent("envoy.local", agent);
    /// ```
    #[inline]
    pub fn with_agent(host: impl Display, agent: ureq::Agent) -> Self {
        let base_url = format!("https://{host}");

        Self { agent, base_url }
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
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let entrez = Entrez::default();
    /// entrez.login_with_env()?;
    /// let token = entrez.generate_token("your-site-name", "your-envoy-serial-number", true)?;
    /// let client = Envoy::new("envoy.local");
    /// client.authenticate(&token)?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[expect(clippy::cognitive_complexity, reason = "Instrumentation macro")]
    #[instrument(skip(self, token), level = "debug")]
    pub fn authenticate(&self, token: impl Display) -> Result<()> {
        debug!("Authenticating Envoy via JWT");

        let endpoint = format!("{}/auth/check_jwt", self.base_url);
        debug!("GET {endpoint}");

        let mut response = self
            .agent
            .get(&endpoint)
            .header("Authorization", format!("Bearer {token}"))
            .call()?;
        debug!("Status code: {}", response.status());

        let body = response.body_mut().read_to_string()?;

        if response.status() == 200 && body.contains("Valid token") {
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
    /// Returns an error if the request fails or the device does not respond correctly.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::{Envoy, models::PowerState};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Envoy::new("envoy.local");
    /// client.set_power_state("603980032", PowerState::Off)?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[instrument(skip(self, serial, state), level = "debug")]
    #[expect(clippy::cognitive_complexity, reason = "Parsing response")]
    pub fn set_power_state(&self, serial: impl Display, state: PowerState) -> Result<()> {
        debug!(?state, "Setting power state");

        let endpoint = format!("{}/ivp/mod/{}/mode/power", self.base_url, serial);
        debug!("PUT {endpoint}");

        // Build the JSON payload
        let payload = format!(r#"{{"length":1,"arr":[{}]}}"#, state.payload_value());

        let response = self
            .agent
            .put(&endpoint)
            .header(
                "Content-Type",
                "application/x-www-form-urlencoded; charset=UTF-8",
            )
            .send(payload)?;

        debug!("Status code: {}", response.status());

        // The endpoint returns 204 No Content on success
        if response.status() == 204 {
            debug!("Power state set successfully");
            return Ok(());
        }

        Err(crate::error::EnphaseError::InvalidResponse(format!(
            "Failed to set power state: HTTP {}",
            response.status()
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
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Envoy::new("envoy.local");
    /// let is_on = client.get_power_state("603980032")?;
    /// println!("Power is {}", if is_on { "on" } else { "off" });
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[instrument(skip(self, serial), level = "debug")]
    #[expect(clippy::cognitive_complexity, reason = "Instrumentation macro")]
    pub fn get_power_state(&self, serial: impl Display) -> Result<bool> {
        debug!("Getting power state");

        let endpoint = format!("{}/ivp/mod/{}/mode/power", self.base_url, serial);
        debug!("GET {endpoint}");

        let mut response = self
            .agent
            .get(&endpoint)
            .header("Accept", "application/json, text/javascript, */*; q=0.01")
            .call()?;

        debug!("Status code: {}", response.status());

        let body = response.body_mut().read_to_string()?;
        debug!("Response body: {}", body);

        let status: PowerStatusResponse = serde_json::from_str(&body)?;
        debug!(?status, "Parsed power status");

        // powerForcedOff: true means power is OFF, so we invert it
        Ok(!status.power_forced_off)
    }
}
