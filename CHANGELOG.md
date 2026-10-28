# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- markdownlint-disable -->
## [1.1.0](https://github.com/JP-Ellis/enphase-api/compare/enphase-api-v1.0.0...enphase-api-v1.1.0) - _2026-10-28_

### 🚀 Features

-   _(bridge)_ Classify Envoy errors as reachable vs unreachable
-   _(bridge)_ Scaffold enphase-bridge workspace crate
-   _(envoy)_ Add optional 0600 on-disk token cache
-   _(envoy)_ Self-authenticating client with refresh on Unauthorized
-   _(envoy)_ Add inverter enumeration via /inventory.json
-   _(envoy)_ Map HTTP 401 to a distinct Unauthorized error
-   _(envoy)_ Add grid-profile package upload
-   _(envoy)_ Add grid-profile read and select
-   _(envoy)_ Add DPEL export-limit control

### 🐛 Bug Fixes

-   _(envoy)_ Gate token-cache 0600 hardening behind cfg(unix)
-   _(envoy)_ Make token-cache writes best-effort and redact credentials in Debug

### 🚜 Refactor

-   _(envoy)_ Escape grid-profile id and tighten DPEL tests

### ⚙️ Miscellaneous Tasks

-   Measure coverage across the whole workspace
-   Allow 80-char subjects and 100-char bodies
-   _(renovate)_ Group the rust toolchain pins
-   Add codecov config with tolerance
-   Pin the rust toolchain
-   Silence new clippy lints
-   Update to tombi and prek


## [1.0.0](https://github.com/JP-Ellis/enphase-api/releases/tag/v1.0.0) - _2026-03-17_

### 🚀 Features

-   [**breaking**] Change api to be async
-   Add fetching of power state
-   Initial client implementation

### 📚 Documentation

-   Update readme with async api
-   Update readme

### 🧪 Testing

-   Add unit tests from fixtures

### ⚙️ Miscellaneous Tasks

-   Add release-plz workflow and configuration
-   Remove multiple crate lint
-   _(ci)_ Replace pre-commit with prek
-   Update biome schema
-   Remove unused prek hooks
-   Fix new clippy lints
-   _(ci)_ Disable tests for now
-   Initial commit
