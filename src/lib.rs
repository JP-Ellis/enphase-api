//! # Rust client for Enphase/Envoy systems.

#![expect(clippy::pub_use, reason = "Root API exports for convenience")]

extern crate alloc;

mod client;
mod error;
pub mod models;

// Export main clients
pub use client::{auth::Credentials, entrez::Entrez, envoy::Envoy};

// Export error types (both names for compatibility)
pub use error::{EnphaseError, Result};
