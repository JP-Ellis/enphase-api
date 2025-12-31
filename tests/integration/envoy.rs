//! Integration tests for the Envoy client
//!
//! These tests require valid Enphase credentials and network access to both the
//! Enphase Entrez service and an Envoy device. They are skipped if the required
//! environment variables are not set.

use enphase_api::{Entrez, Envoy, models::PowerState};

/// Check if credentials are available for testing
fn has_credentials() -> Result<(), Box<dyn core::error::Error>> {
    std::env::var("ENTREZ_USERNAME")?;
    std::env::var("ENTREZ_PASSWORD")?;
    std::env::var("ENVOY_HOST")?;
    std::env::var("ENVOY_NAME")?;
    std::env::var("ENVOY_SERIAL_NUMBER")?;
    Ok(())
}

#[test]
#[ignore = "Requires local Envoy device and Enphase credentials"]
fn authenticate_and_set_power_state() -> Result<(), Box<dyn core::error::Error>> {
    has_credentials()?;

    // Get credentials from environment
    let envoy_host = std::env::var("ENVOY_HOST")?;
    let envoy_name = std::env::var("ENVOY_NAME")?;
    let envoy_serial = std::env::var("ENVOY_SERIAL_NUMBER")?;

    // Step 1: Create Entrez client and generate JWT token
    let entrez = Entrez::default();
    entrez.login_with_env()?;
    let token = entrez.generate_token(&envoy_name, &envoy_serial, true)?;
    println!("Generated JWT token for authentication");

    // Step 2: Create Envoy client and authenticate with the token
    let envoy = Envoy::new(&envoy_host);
    envoy.authenticate(&token)?;
    println!("Successfully authenticated with Envoy device");

    // Step 3: Set power production to On
    envoy.set_power_state("603980032", PowerState::On)?;
    println!("Successfully set power state to On");

    Ok(())
}

#[test]
#[ignore = "Requires local Envoy device and Enphase credentials"]
fn authenticate_and_get_power_state() -> Result<(), Box<dyn core::error::Error>> {
    has_credentials()?;

    // Get credentials from environment
    let envoy_host = std::env::var("ENVOY_HOST")?;
    let envoy_name = std::env::var("ENVOY_NAME")?;
    let envoy_serial = std::env::var("ENVOY_SERIAL_NUMBER")?;

    // Step 1: Create Entrez client and generate JWT token
    let entrez = Entrez::default();
    entrez.login_with_env()?;
    let token = entrez.generate_token(&envoy_name, &envoy_serial, true)?;
    println!("Generated JWT token for authentication");

    // Step 2: Create Envoy client and authenticate with the token
    let envoy = Envoy::new(&envoy_host);
    envoy.authenticate(&token)?;
    println!("Successfully authenticated with Envoy device");

    // Step 3: Get current power state
    let is_on = envoy.get_power_state("603980032")?;
    println!("Power is {}", if is_on { "on" } else { "off" });

    // Verify the response is a valid boolean
    println!("Successfully retrieved power state");

    Ok(())
}
