//! Reachability classification for Envoy errors.
//!
//! The bridge must tell "the whole box is unreachable" (a refused connection, a
//! timeout, a dead service) apart from "the box answered but one operation
//! failed" (an HTTP status or an unparsable body). The former drives every
//! control entity offline and the reachability sensor off; the latter drops only
//! the one affected entity. [`classify`] draws that line from the library's
//! [`EnphaseError`] taxonomy.

use enphase_api::EnphaseError;

/// Whether the Envoy was reachable when an operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reachability {
    /// The box answered (or no connection-level failure is implicated); the
    /// operation failed for some other reason.
    Reachable,
    /// The box could not be reached at all: a refused connection, a timeout, or
    /// a request that never got a response.
    Unreachable,
}

/// Classify an error as connection-level unreachable or operation-failed.
///
/// Only a connection-level `reqwest` failure carried by
/// [`EnphaseError::Http`] counts as [`Unreachable`](Reachability::Unreachable):
/// a refused connection, a timeout, or a request that never received a response.
/// Every other variant — an HTTP status, an unparsable body, an I/O or
/// configuration error — and any non-[`EnphaseError`] means the box answered (or
/// the failure is ambiguous), so it is [`Reachable`](Reachability::Reachable):
/// the less-destructive reading drops one entity, not all.
///
/// # Arguments
///
/// * `error` - The error to classify.
///
/// # Returns
///
/// [`Reachability::Unreachable`] for a connection-level failure, otherwise
/// [`Reachability::Reachable`].
#[inline]
#[must_use]
pub fn classify(error: &anyhow::Error) -> Reachability {
    match error.downcast_ref::<EnphaseError>() {
        Some(EnphaseError::Http(re)) => classify_http(re),
        _ => Reachability::Reachable,
    }
}

/// Classify a `reqwest` error by whether the box answered.
///
/// # Arguments
///
/// * `re` - The underlying `reqwest` error.
///
/// # Returns
///
/// [`Reachability::Unreachable`] for a connect, timeout, or no-response request
/// failure; [`Reachability::Reachable`] once a response (including an error
/// status) was received.
fn classify_http(re: &reqwest::Error) -> Reachability {
    // A request-kind error is one that never got a response, so it is a distinct
    // kind from a status error — no `!is_status()` guard is needed alongside it.
    if re.is_connect() || re.is_timeout() || re.is_request() {
        Reachability::Unreachable
    } else {
        Reachability::Reachable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn classify_operation_failures_are_reachable() {
        for err in [
            EnphaseError::InvalidResponse("bad".to_owned()),
            EnphaseError::Unauthorized("401".to_owned()),
            EnphaseError::JsonError(serde_json::from_str::<i32>("x").expect_err("invalid json")),
        ] {
            assert_eq!(classify(&anyhow::Error::new(err)), Reachability::Reachable);
        }
    }

    #[test]
    fn classify_non_enphase_error_defaults_reachable() {
        // The less-destructive reading: an ambiguous error drops one entity, not all.
        assert_eq!(
            classify(&anyhow::anyhow!("opaque")),
            Reachability::Reachable
        );
    }
}
