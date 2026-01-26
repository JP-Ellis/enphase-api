//! # Error types for the Enphase API client
//!
//! This module contains all error types and handling for the Enphase API
//! client.

/// Error types that can occur when using the Enphase API client.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EnphaseError {
    /// HTTP request error from reqwest.
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// Invalid response from the API.
    #[error("Invalid API response: {0}")]
    InvalidResponse(String),

    /// Authentication failed.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// JSON parsing error.
    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Result type for Enphase API operations.
pub type Result<T> = core::result::Result<T, EnphaseError>;
