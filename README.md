# Enphase API

<!-- markdownlint-disable no-inline-html -->
<div align="center"><table>
    <tr>
        <td>Package</td>
        <td>
            <a href="https://crates.io/crates/enphase-api"><img src="https://img.shields.io/crates/v/enphase-api.svg" alt="Version"></a>
            <a href="https://crates.io/crates/enphase-api"><img src="https://img.shields.io/crates/d/enphase-api.svg" alt="Downloads"></a>
            <a href="https://docs.rs/enphase-api"><img src="https://docs.rs/enphase-api/badge.svg" alt="Documentation"></a>
        </td>
    </tr>
    <tr>
        <td>CI/CD</td>
        <td>
            <a
                href="https://github.com/JP-Ellis/enphase-api/actions/workflows/deploy.yml"><img
                src="https://img.shields.io/github/actions/workflow/status/JP-Ellis/enphase-api/deploy.yml?branch=main&label=CI"
                alt="CI Status"></a>
            <a
                href="https://github.com/JP-Ellis/enphase-api/actions/workflows/test.yml"><img
                src="https://img.shields.io/github/actions/workflow/status/JP-Ellis/enphase-api/test.yml?branch=main&label=tests"
                alt="Test Status"></a>
        </td>
    </tr>
    <tr>
        <td>Meta</td>
        <td>
            <a
                href="https://github.com/rust-lang/cargo"><img
                src="https://img.shields.io/badge/🦀-Cargo-blue.svg"
                alt="Cargo project"></a>
            <a href="https://github.com/rust-lang/rustfmt"><img
                src="https://img.shields.io/badge/code%20style-rustfmt-brightgreen.svg"
                alt="Code style - rustfmt"></a>
            <a href="https://github.com/rust-lang/rust-clippy"><img
                src="https://img.shields.io/badge/linting-clippy-blue.svg"
                alt="Linting - Clippy"></a>
            <a
                href="https://opensource.org/licenses/MIT"><img
                src="https://img.shields.io/badge/License-MIT-green.svg"
                alt="License"></a>
        </td>
    </tr>
    <tr>
        <td>Community</td>
        <td>
            <a
                href="https://github.com/JP-Ellis/enphase-api/issues"><img
                src="https://img.shields.io/github/issues/JP-Ellis/enphase-api.svg"
                alt="Issues"></a>
            <a
                href="https://github.com/JP-Ellis/enphase-api/discussions"><img
                src="https://img.shields.io/github/discussions/JP-Ellis/enphase-api.svg"
                alt="Discussions"></a>
            <a
                href="https://github.com/JP-Ellis/enphase-api"><img
                src="https://img.shields.io/github/stars/JP-Ellis/enphase-api.svg?style=social"
                alt="GitHub Stars"></a>
        </td>
    </tr>
</table></div>
<!-- markdownlint-enable no-inline-html -->

A Rust client library for interacting with Enphase solar systems via the Entrez cloud service and local Envoy gateway.

> [!NOTE]
>
> This library is in early development. Currently, only authentication and basic power control are implemented. Additional API endpoints will be added over time. **Contributions are very welcome!**

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
enphase-api = "~1"
```

## Quick Start

```rust
use enphase_api::{Entrez, Envoy, PowerState};

// Step 1: Authenticate with Enphase Entrez and get a JWT token
let entrez = Entrez::default();
entrez.login("your-email@example.com", "your-password")?;
let token = entrez.generate_token("your-site-name", "your-envoy-serial", true)?;

// Step 2: Connect to your local Envoy gateway
let envoy = Envoy::new("envoy.local");
envoy.authenticate(&token)?;

// Step 3: Control your system (example: set power state)
envoy.set_power_state("device-serial-number", PowerState::On)?;
```

The library supports Enphase's two-step authentication:

1.  **Entrez Cloud Service**: Authenticate with your Enphase account to generate JWT tokens
2.  **Local Envoy Gateway**: Use the JWT token to access your local Envoy device

The client then remembers the authentication state for subsequent API calls. Note that the JWT token has a limited lifespan and re-authentication may be necessary.

### Using Environment Variables

For convenience, you can use environment variables for authentication:

```rust
use enphase_api::{Entrez, Envoy};

// Set these environment variables:
// - ENTREZ_USERNAME
// - ENTREZ_PASSWORD
let entrez = Entrez::default();
entrez.login_with_env()?;

let token = entrez.generate_token("your-site-name", "your-envoy-serial", true)?;

let envoy = Envoy::new("envoy.local"); // or use IP address like "192.168.1.100"
envoy.authenticate(&token)?;
```

## Current API Coverage

This library is in early development. Currently implemented:

### Entrez Client

-   User authentication ([`login`](src/client/entrez.rs), [`login_with_env`](src/client/entrez.rs))
-   JWT token generation for Envoy devices ([`generate_token`](src/client/entrez.rs))

### Envoy Client

-   JWT authentication ([`authenticate`](src/client/envoy.rs))
-   Power state control ([`set_power_state`](src/client/envoy.rs))

### Planned Features

The following features are planned for future releases:

-   **Production API**: Real-time solar production data
-   **Consumption API**: Energy consumption monitoring
-   **Inverter API**: Individual inverter performance data
-   **System API**: Overall system information and status

**We welcome contributions!** If you need a specific API endpoint, please consider opening an issue or submitting a pull request.

## Documentation

-   [API Documentation](https://docs.rs/enphase-api)
-   [Enphase Website](https://enphase.com/)
-   [Enphase Developer Portal](https://developer.enphase.com/)

## Contributing

We welcome contributions! This library is in active development and there are many API endpoints still to implement. Please see our [Contributing Guide](./CONTRIBUTING.md) for details on:

-   Setting up the development environment
-   Running tests and examples
-   Code style and formatting guidelines
-   Submitting pull requests

If you'd like to add support for additional API endpoints, please feel free to open an issue or submit a pull request!

## Testing

Run the test suite:

```bash
# Run unit tests
cargo test

# Run tests with nextest (faster)
cargo nextest run

# Run integration tests (requires credentials)
cargo test --test integration -- --ignored
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Support

-   📚 [Documentation](https://docs.rs/enphase-api)
-   🐛 [Issue Tracker](https://github.com/JP-Ellis/enphase-api/issues)
-   💬 [Discussions](https://github.com/JP-Ellis/enphase-api/discussions)
