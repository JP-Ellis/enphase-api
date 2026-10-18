//! Integration tests for the Envoy client.
//!
//! These tests require valid Enphase credentials and network access to both the
//! Enphase Entrez service and an Envoy device. They are skipped if the required
//! environment variables are not set.

use enphase_api::{Entrez, Envoy, models::PowerState};

/// Check if credentials are available for testing.
fn has_credentials() -> Result<(), Box<dyn core::error::Error>> {
    std::env::var("ENTREZ_USERNAME")?;
    std::env::var("ENTREZ_PASSWORD")?;
    std::env::var("ENVOY_HOST")?;
    std::env::var("ENVOY_NAME")?;
    std::env::var("ENVOY_SERIAL_NUMBER")?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires local Envoy device and Enphase credentials"]
async fn authenticate_and_set_power_state() -> Result<(), Box<dyn core::error::Error>> {
    has_credentials()?;

    // Get credentials from environment
    let envoy_host = std::env::var("ENVOY_HOST")?;
    let envoy_name = std::env::var("ENVOY_NAME")?;
    let envoy_serial = std::env::var("ENVOY_SERIAL_NUMBER")?;

    // Step 1: Create Entrez client and generate JWT token
    let entrez = Entrez::default();
    entrez.login_with_env().await?;
    let token = entrez
        .generate_token(&envoy_name, &envoy_serial, true)
        .await?;
    println!("Generated JWT token for authentication");

    // Step 2: Create Envoy client and authenticate with the token
    let envoy = Envoy::new(&envoy_host);
    envoy.authenticate(&token).await?;
    println!("Successfully authenticated with Envoy device");

    // Step 3: Set power production to On
    envoy.set_power_state("603980032", PowerState::On).await?;
    println!("Successfully set power state to On");

    Ok(())
}

#[tokio::test]
#[ignore = "Requires local Envoy device and Enphase credentials"]
async fn authenticate_and_get_power_state() -> Result<(), Box<dyn core::error::Error>> {
    has_credentials()?;

    // Get credentials from environment
    let envoy_host = std::env::var("ENVOY_HOST")?;
    let envoy_name = std::env::var("ENVOY_NAME")?;
    let envoy_serial = std::env::var("ENVOY_SERIAL_NUMBER")?;

    // Step 1: Create Entrez client and generate JWT token
    let entrez = Entrez::default();
    entrez.login_with_env().await?;
    let token = entrez
        .generate_token(&envoy_name, &envoy_serial, true)
        .await?;
    println!("Generated JWT token for authentication");

    // Step 2: Create Envoy client and authenticate with the token
    let envoy = Envoy::new(&envoy_host);
    envoy.authenticate(&token).await?;
    println!("Successfully authenticated with Envoy device");

    // Step 3: Get current power state
    let is_on = envoy.get_power_state("603980032").await?;
    println!("Power is {}", if is_on { "on" } else { "off" });

    // Verify the response is a valid boolean
    println!("Successfully retrieved power state");

    Ok(())
}

#[tokio::test]
#[ignore = "Requires local Envoy device and Enphase credentials"]
async fn authenticate_and_set_export_limit() -> Result<(), Box<dyn core::error::Error>> {
    has_credentials()?;

    let envoy_host = std::env::var("ENVOY_HOST")?;
    let envoy_name = std::env::var("ENVOY_NAME")?;
    let envoy_serial = std::env::var("ENVOY_SERIAL_NUMBER")?;

    // Step 1: Create Entrez client and generate JWT token
    let entrez = Entrez::default();
    entrez.login_with_env().await?;
    let token = entrez
        .generate_token(&envoy_name, &envoy_serial, true)
        .await?;
    println!("Generated JWT token for authentication");

    // Step 2: Create Envoy client and authenticate with the token
    let envoy = Envoy::new(&envoy_host);
    envoy.authenticate(&token).await?;
    println!("Successfully authenticated with Envoy device");

    // Step 3: Set zero-export and read it back.
    //
    // TODO(phase-2): verify against the device — the real success status code
    // of POST /ivp/ss/dpel and the exact GET body shape are unverified; the
    // fixtures encode the documented shape, not a captured response.
    envoy.set_export_limit(0.0).await?;
    println!("Successfully set export limit to 0 W (zero export)");

    let settings = envoy.get_dpel().await?;
    println!("Read back DPEL settings: {settings:?}");

    Ok(())
}

#[tokio::test]
#[ignore = "Requires local Envoy device and Enphase credentials"]
async fn authenticate_and_get_grid_profiles() -> Result<(), Box<dyn core::error::Error>> {
    has_credentials()?;

    let envoy_host = std::env::var("ENVOY_HOST")?;
    let envoy_name = std::env::var("ENVOY_NAME")?;
    let envoy_serial = std::env::var("ENVOY_SERIAL_NUMBER")?;

    // Step 1: Create Entrez client and generate JWT token
    let entrez = Entrez::default();
    entrez.login_with_env().await?;
    let token = entrez
        .generate_token(&envoy_name, &envoy_serial, true)
        .await?;
    println!("Generated JWT token for authentication");

    // Step 2: Create Envoy client and authenticate with the token
    let envoy = Envoy::new(&envoy_host);
    envoy.authenticate(&token).await?;
    println!("Successfully authenticated with Envoy device");

    // Step 3: List the grid profiles.
    //
    // TODO(phase-2): confirm the real index.json field names against the
    // device; the model assumes selected_profile / selected_profile_id /
    // profiles[].profile_id.
    let index = envoy.get_grid_profiles().await?;
    println!("Active profile: {}", index.selected_profile_id);
    println!("Available profiles: {}", index.profiles.len());

    Ok(())
}

#[tokio::test]
#[ignore = "Requires local Envoy device and Enphase credentials"]
async fn authenticate_and_get_inverters() -> Result<(), Box<dyn core::error::Error>> {
    has_credentials()?;

    let envoy_host = std::env::var("ENVOY_HOST")?;
    let envoy_name = std::env::var("ENVOY_NAME")?;
    let envoy_serial = std::env::var("ENVOY_SERIAL_NUMBER")?;

    // Step 1: Create Entrez client and generate JWT token
    let entrez = Entrez::default();
    entrez.login_with_env().await?;
    let token = entrez
        .generate_token(&envoy_name, &envoy_serial, true)
        .await?;
    println!("Generated JWT token for authentication");

    // Step 2: Create Envoy client and authenticate with the token
    let envoy = Envoy::new(&envoy_host);
    envoy.authenticate(&token).await?;
    println!("Successfully authenticated with Envoy device");

    // Step 3: Enumerate the array's inverters.
    //
    // TODO(phase-2): confirm the real /inventory.json section type and serial
    // field names against the device; the model assumes a top-level array of
    // sections with type == "PCU" and devices[].serial_num.
    let serials = envoy.get_inverters().await?;
    println!("Found {} inverters: {serials:?}", serials.len());

    Ok(())
}

#[tokio::test]
#[ignore = "Requires local Envoy device, Enphase credentials, and ENVOY_PROFILE_PACKAGE"]
async fn authenticate_and_upload_grid_profile() -> Result<(), Box<dyn core::error::Error>> {
    has_credentials()?;

    // The profile package path is required for this test; skip (pass) if unset.
    let Ok(package_path) = std::env::var("ENVOY_PROFILE_PACKAGE") else {
        println!("ENVOY_PROFILE_PACKAGE not set; skipping");
        return Ok(());
    };

    let envoy_host = std::env::var("ENVOY_HOST")?;
    let envoy_name = std::env::var("ENVOY_NAME")?;
    let envoy_serial = std::env::var("ENVOY_SERIAL_NUMBER")?;

    // Step 1: Create Entrez client and generate JWT token
    let entrez = Entrez::default();
    entrez.login_with_env().await?;
    let token = entrez
        .generate_token(&envoy_name, &envoy_serial, true)
        .await?;
    println!("Generated JWT token for authentication");

    // Step 2: Create Envoy client and authenticate with the token
    let envoy = Envoy::new(&envoy_host);
    envoy.authenticate(&token).await?;
    println!("Successfully authenticated with Envoy device");

    // Step 3: Upload the profile package, then select it.
    //
    // This is the offline profile-switch path (spec phase 2): on 8.x only one
    // profile persists, so a package must be uploaded immediately before it can
    // be selected. ENVOY_PROFILE_ID, if set, is the id to select afterwards.
    let package = std::fs::read(&package_path)?;
    envoy.upload_grid_profile(package).await?;
    println!("Uploaded grid profile package from {package_path}");

    if let Ok(profile_id) = std::env::var("ENVOY_PROFILE_ID") {
        envoy.select_grid_profile(&profile_id).await?;
        println!("Selected grid profile {profile_id}");
    }

    Ok(())
}
