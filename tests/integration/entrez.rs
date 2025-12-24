//! Integration tests for the Entrez client
//!
//! These tests require valid Enphase credentials and network access to the
//! Enphase Entrez service. They are skipped if the required environment
//! variables are not set.

use enphase_api::Entrez;

/// Check if credentials are available for testing
fn has_credentials() -> Result<(), Box<dyn core::error::Error>> {
    std::env::var("ENTREZ_USERNAME")?;
    std::env::var("ENTREZ_PASSWORD")?;
    Ok(())
}

#[test]
#[ignore = "Requires Enphase credentials"]
fn login_and_generate_token() -> Result<(), Box<dyn core::error::Error>> {
    has_credentials()?;

    let site_name = std::env::var("ENVOY_NAME")?;
    let serial_number = std::env::var("ENVOY_SERIAL_NUMBER")?;
    let client = Entrez::default();

    client.login_with_env()?;
    let token = client.generate_token(&site_name, &serial_number, true)?;
    println!("Generated token: {token}");

    Ok(())
}
