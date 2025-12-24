# Enphase API

<!-- markdownlint-disable no-inline-html -->
<div align="center">
    <img src="logo.svg" alt="Enphase API Logo" height="200" align="left" hspace="20">
    <span>
        <b>
            A Rust client library for the Enphase/Envoy API, providing easy access to local solar energy data.
        </b>
    </span>
</div>

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

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
enphase-api = "~1"
```

## Quick Start

```rust
use enphase_api::Enphase;

// Create a client
let client = Enphase::builder()
    .host("envoy.local") // Your Envoy gateway address
    .build()?;

// Get production data
let production = client.production()?;
println!("Current production: {} W", production.watts_now);
```

## Authentication

You'll need to configure your Envoy gateway address. Authentication methods vary by Envoy model:

### Basic Configuration

```rust
let client = Enphase::builder()
    .host("envoy.local") // or IP address like "192.168.1.100"
    .build()?;
```

### With Authentication (if required)

```rust
let client = Enphase::builder()
    .host("envoy.local")
    .username("envoy")
    .password("your-password")
    .build()?;
```

## Examples

Check out the [examples directory](./examples/) for comprehensive usage examples. You can run them directly using Cargo and they will demonstrate various API features:

```bash
cargo run --example production
```

## API Coverage

This library provides access to:

-   **Production API**: Real-time solar production data
-   **Consumption API**: Energy consumption monitoring
-   **Inverter API**: Individual inverter performance data
-   **System API**: Overall system information and status

For detailed API documentation, visit the [Enphase Developer Portal](https://developer.enphase.com/).

## Documentation

-   [API Documentation](https://docs.rs/enphase-api)
-   [Enphase Website](https://enphase.com/)
-   [Enphase Developer Portal](https://developer.enphase.com/)

## Contributing

We welcome contributions! Please see our [Contributing Guide](./CONTRIBUTING.md) for details on:

-   Setting up the development environment
-   Running tests and examples
-   Code style and formatting guidelines
-   Submitting pull requests

## Testing

Run the test suite:

```bash
# Run tests with nextest (faster)
cargo nextest run

# Run integration tests
cargo test --test integration
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Support

-   📚 [Documentation](https://docs.rs/enphase-api)
-   🐛 [Issue Tracker](https://github.com/JP-Ellis/enphase-api/issues)
-   💬 [Discussions](https://github.com/JP-Ellis/enphase-api/discussions)
