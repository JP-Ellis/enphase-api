//! # Self-authenticating Envoy support
//!
//! This module holds the credentials and token state a self-authenticating
//! [`Envoy`](crate::Envoy) uses to acquire a JWT via Entrez, hold it, and
//! refresh it transparently when a request is rejected with
//! [`Unauthorized`](crate::error::EnphaseError::Unauthorized).

use std::sync::Mutex;

use crate::client::entrez::Entrez;
use crate::error::Result;

/// Credentials for acquiring an Envoy JWT via the Enphase Entrez service.
///
/// Combines the Enphase account login with the site and serial the token is
/// minted for. Fields are private; construct with [`Credentials::new`] and
/// adjust the commissioned flag with [`Credentials::commissioned`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Credentials {
    /// Enphase account username.
    username: String,
    /// Enphase account password.
    password: String,
    /// The site name the token is generated for.
    site_name: String,
    /// The Envoy serial number the token is generated for.
    serial_number: String,
    /// Whether the device is commissioned.
    commissioned: bool,
}

impl Credentials {
    /// Create credentials for token acquisition.
    ///
    /// The commissioned flag defaults to `true`; override it with
    /// [`commissioned`](Self::commissioned).
    ///
    /// # Arguments
    ///
    /// * `username` - Enphase account username.
    /// * `password` - Enphase account password.
    /// * `site_name` - The site name the token is generated for.
    /// * `serial_number` - The Envoy serial number the token is generated for.
    ///
    /// # Returns
    ///
    /// Returns a new [`Credentials`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Credentials;
    ///
    /// let creds = Credentials::new("user@example.com", "password", "My Site", "121212121212");
    /// ```
    #[inline]
    pub fn new(
        username: impl Into<String>,
        password: impl Into<String>,
        site_name: impl Into<String>,
        serial_number: impl Into<String>,
    ) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            site_name: site_name.into(),
            serial_number: serial_number.into(),
            commissioned: true,
        }
    }

    /// Set whether the device is commissioned.
    ///
    /// # Arguments
    ///
    /// * `value` - `true` if the device is commissioned, `false` otherwise.
    ///
    /// # Returns
    ///
    /// Returns the updated [`Credentials`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// use enphase_api::Credentials;
    ///
    /// let creds = Credentials::new("user@example.com", "password", "My Site", "121212121212")
    ///     .commissioned(false);
    /// ```
    #[inline]
    #[must_use]
    pub fn commissioned(mut self, value: bool) -> Self {
        self.commissioned = value;
        self
    }
}

/// The auth source and cached token for a self-authenticating Envoy.
///
/// Holds the Entrez client and credentials used to mint a JWT, plus the token
/// most recently obtained. Shared behind an `Arc` so cloning an `Envoy` shares
/// one token.
#[derive(Debug)]
pub(crate) struct AuthState {
    /// The Entrez client used to log in and generate tokens.
    entrez: Entrez,
    /// The credentials used to acquire a token.
    credentials: Credentials,
    /// The most recently acquired token, if any.
    token: Mutex<Option<String>>,
}

impl AuthState {
    /// Create a new auth state with no token yet acquired.
    ///
    /// # Arguments
    ///
    /// * `entrez` - The Entrez client used to log in and generate tokens.
    /// * `credentials` - The credentials used to acquire a token.
    ///
    /// # Returns
    ///
    /// Returns a new [`AuthState`].
    pub(crate) fn new(entrez: Entrez, credentials: Credentials) -> Self {
        Self {
            entrez,
            credentials,
            token: Mutex::new(None),
        }
    }

    /// Return the currently held token, if any.
    ///
    /// The lock is released before returning, so callers never hold it across
    /// an `.await`.
    ///
    /// # Returns
    ///
    /// Returns a clone of the held token, or `None` if none has been acquired.
    #[expect(
        clippy::expect_used,
        reason = "a poisoned token mutex indicates an unrecoverable bug"
    )]
    pub(crate) fn token(&self) -> Option<String> {
        self.token.lock().expect("token mutex poisoned").clone()
    }

    /// Store a freshly acquired token.
    ///
    /// # Arguments
    ///
    /// * `token` - The token to store.
    #[expect(
        clippy::expect_used,
        reason = "a poisoned token mutex indicates an unrecoverable bug"
    )]
    pub(crate) fn set_token(&self, token: String) {
        *self.token.lock().expect("token mutex poisoned") = Some(token);
    }

    /// Log in to Entrez and generate a fresh Envoy JWT.
    ///
    /// Does not store the token or validate it against the Envoy; the caller
    /// authenticates and stores it.
    ///
    /// # Returns
    ///
    /// Returns the freshly minted JWT.
    ///
    /// # Errors
    ///
    /// Returns an error if the Entrez login or token generation fails.
    pub(crate) async fn acquire_token(&self) -> Result<String> {
        self.entrez
            .login(&self.credentials.username, &self.credentials.password)
            .await?;
        self.entrez
            .generate_token(
                &self.credentials.site_name,
                &self.credentials.serial_number,
                self.credentials.commissioned,
            )
            .await
    }
}
