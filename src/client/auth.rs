//! # Self-authenticating Envoy support
//!
//! This module holds the credentials and token state a self-authenticating
//! [`Envoy`](crate::Envoy) uses to acquire a JWT via Entrez, hold it, and
//! refresh it transparently when a request is rejected with
//! [`Unauthorized`](crate::error::EnphaseError::Unauthorized).

use std::path::Path;
use std::sync::Mutex;

use crate::client::entrez::Entrez;
use crate::error::Result;

/// Read a cached token from `path`, best-effort.
///
/// A missing or unreadable file, or an empty/whitespace-only body, yields
/// `None` — the caller then falls back to a full login. Read errors are never
/// surfaced: a broken cache must not break authentication.
///
/// # Arguments
///
/// * `path` - The cache file to read.
///
/// # Returns
///
/// Returns the trimmed cached token, or `None` if none is available.
pub(crate) fn load_cached_token(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|contents| contents.trim().to_owned())
        .filter(|token| !token.is_empty())
}

/// Write `token` to `path`, creating the file with mode `0600` on Unix.
///
/// The token is an installer-grade credential, so on Unix the file is never
/// world-readable: it is created `0600` and its permissions are reasserted
/// after writing to cover the overwrite-an-existing-file case. The bridge
/// targets Linux; on non-Unix platforms the token is written without POSIX
/// permission hardening (restrict the containing directory yourself if the
/// path is shared).
///
/// # Arguments
///
/// * `path` - The cache file to write.
/// * `token` - The token to persist.
///
/// # Errors
///
/// Returns an error if the file cannot be created or written.
pub(crate) fn save_cached_token(path: &Path, token: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(token.as_bytes())?;
        // `mode` above only applies when the file is created; reassert 0600 so
        // an overwrite of a pre-existing file is tightened too.
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, token.as_bytes())?;
    }
    Ok(())
}

/// Credentials for acquiring an Envoy JWT via the Enphase Entrez service.
///
/// Combines the Enphase account login with the site and serial the token is
/// minted for. Fields are private; construct with [`Credentials::new`] and
/// adjust the commissioned flag with [`Credentials::commissioned`].
#[derive(Clone)]
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

impl core::fmt::Debug for Credentials {
    /// Redacts the password so it never reaches logs via a debug print.
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("site_name", &self.site_name)
            .field("serial_number", &self.serial_number)
            .field("commissioned", &self.commissioned)
            .finish()
    }
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
pub(crate) struct AuthState {
    /// The Entrez client used to log in and generate tokens.
    entrez: Entrez,
    /// The credentials used to acquire a token.
    credentials: Credentials,
    /// The most recently acquired token, if any.
    token: Mutex<Option<String>>,
}

impl core::fmt::Debug for AuthState {
    /// Reports only whether a token is held, never its value.
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let token_present = self.token.lock().is_ok_and(|guard| guard.is_some());
        f.debug_struct("AuthState")
            .field("entrez", &self.entrez)
            .field("credentials", &self.credentials)
            .field("token_present", &token_present)
            .finish()
    }
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
