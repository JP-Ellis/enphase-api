//! # Enphase Entrez Cloud Service Client
//!
//! This module provides a client for interacting with the Enphase Entrez service,
//! which is used to manage authentication and generate JWT tokens for Envoy devices.
//!
//! The Entrez service is a cloud-based service that provides:
//! - User authentication
//! - JWT token generation for Envoy devices
//! - Site and system information

use crate::error::Result;
use tracing::{debug, instrument};

/// The default base URL for the Enphase Entrez service
const DEFAULT_ENTREZ_URL: &str = "https://entrez.enphaseenergy.com";

/// Main client for the Enphase Entrez service
///
/// This client provides authentication and token generation for accessing
/// Envoy devices via JWT tokens.
#[derive(Debug)]
pub struct Entrez {
    /// HTTP agent for making requests
    agent: ureq::Agent,
    /// Base URL for the Entrez service
    base_url: String,
}

impl Default for Entrez {
    /// Create a new Entrez client with the default URL.
    ///
    /// This connects to the official Enphase Entrez service at
    /// `https://entrez.enphaseenergy.com`.
    #[inline]
    fn default() -> Self {
        Self::new(DEFAULT_ENTREZ_URL)
    }
}

impl Entrez {
    /// Create a new Entrez client with the given URL.
    ///
    /// # Arguments
    ///
    /// * `url` - The base URL of the Entrez service
    ///
    /// # Returns
    ///
    /// Returns a new [`Entrez`] client configured for the given URL.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Entrez;
    ///
    /// let client = Entrez::new("https://entrez.enphaseenergy.com");
    /// ```
    #[inline]
    pub fn new(url: impl Into<String>) -> Self {
        let base_url = url.into();

        Self {
            agent: ureq::agent(),
            base_url,
        }
    }

    /// Create a new Entrez client with the given URL and agent.
    ///
    /// This allows you to provide a custom `ureq::Agent` with specific configuration.
    ///
    /// # Arguments
    ///
    /// * `url` - The base URL of the Entrez service
    /// * `agent` - A configured `ureq::Agent`
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Entrez;
    ///
    /// let agent = ureq::agent();
    /// let client = Entrez::with_agent("https://entrez.enphaseenergy.com", agent);
    /// ```
    #[inline]
    pub fn with_agent(url: impl Into<String>, agent: ureq::Agent) -> Self {
        let base_url = url.into();

        Self { agent, base_url }
    }

    /// Log in to the Enphase Entrez service.
    ///
    /// This authenticates your account and maintains the session for subsequent
    /// API calls. The session is maintained automatically by the HTTP agent.
    ///
    /// # Arguments
    ///
    /// * `username` - Your Enphase account username
    /// * `password` - Your Enphase account password
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if login is successful.
    ///
    /// # Errors
    ///
    /// Returns an error if the login fails due to invalid credentials or network issues.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Entrez;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Entrez::default();
    /// client.login("user@example.com", "password")?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[expect(clippy::cognitive_complexity, reason = "Instrumentation macro")]
    #[instrument(skip(self, username, password), level = "debug")]
    pub fn login(&self, username: impl AsRef<str>, password: impl AsRef<str>) -> Result<()> {
        let username_str = username.as_ref();
        let password_str = password.as_ref();
        debug!("Logging in to Enphase Entrez with {}", username_str);

        let form_data = [
            ("username", username_str),
            ("password", password_str),
            ("authFlow", "entrezSession"),
        ];

        let endpoint = format!("{}{}", self.base_url, "/login");
        debug!("POST {endpoint}");

        let response = self.agent.post(&endpoint).send_form(form_data)?;
        debug!("Status code: {}", response.status());

        Ok(())
    }

    /// Log in to the Enphase Entrez service using environment variables.
    ///
    /// This authenticates your account using credentials from `ENTREZ_USERNAME`
    /// and `ENTREZ_PASSWORD` environment variables. The session is maintained
    /// automatically by the HTTP agent for subsequent API calls.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if login is successful.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The `ENTREZ_USERNAME` or `ENTREZ_PASSWORD` environment variables are not set
    /// - The login fails due to invalid credentials or network issues
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Entrez;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// // Set ENTREZ_USERNAME and ENTREZ_PASSWORD environment variables
    /// let client = Entrez::default();
    /// client.login_with_env()?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn login_with_env(&self) -> Result<()> {
        let username = std::env::var("ENTREZ_USERNAME").map_err(|_e| {
            crate::error::EnphaseError::ConfigurationError(
                "ENTREZ_USERNAME environment variable not set".to_owned(),
            )
        })?;

        let password = std::env::var("ENTREZ_PASSWORD").map_err(|_e| {
            crate::error::EnphaseError::ConfigurationError(
                "ENTREZ_PASSWORD environment variable not set".to_owned(),
            )
        })?;

        self.login(username, password)
    }

    /// Generate a JWT token for accessing an Envoy device.
    ///
    /// This generates a token that can be used to authenticate with a specific
    /// Envoy device. The token is typically valid for a limited time period.
    ///
    /// # Arguments
    ///
    /// * `site_name` - The name of the site
    /// * `serial_number` - The serial number of the Envoy device
    /// * `commissioned` - Whether the device is commissioned (`true`) or not (`false`)
    ///
    /// # Returns
    ///
    /// Returns the JWT token string on success.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails
    /// - The site or serial number is not found
    /// - You are not logged in
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Entrez;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Entrez::default();
    /// client.login("user@example.com", "password")?;
    ///
    /// let token = client.generate_token("My Site", "121212121212", true)?;
    /// println!("Token: {}", token);
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[expect(clippy::cognitive_complexity, reason = "Parsing HTML response")]
    #[instrument(skip(self, site_name, serial_number, commissioned), level = "debug")]
    pub fn generate_token(
        &self,
        site_name: impl AsRef<str>,
        serial_number: impl AsRef<str>,
        commissioned: bool,
    ) -> Result<String> {
        let site_name_str = site_name.as_ref();
        let serial_number_str = serial_number.as_ref();
        debug!(
            "Generating token for site: {}, serial: {}",
            site_name_str, serial_number_str
        );

        // Normalize site name: lowercase and replace spaces with +
        let normalized_site = site_name_str.to_lowercase().replace(' ', "+");

        let endpoint = format!("{}/entrez_tokens", self.base_url);
        debug!("POST {endpoint}");

        let form_data = [
            ("uncommissioned", if commissioned { "on" } else { "off" }),
            ("Site", normalized_site.as_str()),
            ("serialNum", serial_number_str),
        ];

        let mut response = self.agent.post(&endpoint).send_form(form_data)?;
        debug!("Status code: {}", response.status());

        // Read response as plain text to parse HTML
        let response_text = response.body_mut().read_to_string()?;

        // Parse the response HTML to extract the token
        // Look for the textarea with id="JWTToken"
        if let Some((_, rest)) = response_text.split_once(r#"id="JWTToken""#)
            && let Some((_, start_textarea)) = rest.split_once('>')
            && let Some((token_text, _)) = start_textarea.split_once("</textarea>")
        {
            let token = token_text.trim().to_owned();

            if !token.is_empty() {
                debug!("Token generated successfully");
                return Ok(token);
            }
        }

        Err(crate::error::EnphaseError::InvalidResponse(
            "Failed to extract token from response".to_owned(),
        ))
    }
}
