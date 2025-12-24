//! # Rust client for Enphase/Envoy systems.

#![expect(clippy::pub_use, reason = "Root API exports for convenience")]

mod client;
mod error;
pub mod models;

// Export main clients
pub use client::{entrez::Entrez, envoy::Envoy};

// Export error types (both names for compatibility)
pub use error::{EnphaseError, Result};
