# COMEBACKHERE Contracts

[![WASM Build Check](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/build.yml/badge.svg)](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/build.yml)
[![Test](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/test.yml/badge.svg)](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/test.yml)
[![Lint](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/lint.yml/badge.svg)](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/lint.yml)
[![Format](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/fmt.yml/badge.svg)](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/fmt.yml)
[![Pre-commit](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/pre-commit.yml/badge.svg)](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/pre-commit.yml)
[![Coverage](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/coverage.yml/badge.svg)](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/coverage.yml)
[![Contract Size](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/contract-size.yml/badge.svg)](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/contract-size.yml)
[![ABI Snapshot Drift Check](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/abi-drift-check.yml/badge.svg)](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/abi-drift-check.yml)
[![Deny](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/deny.yml/badge.svg)](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/deny.yml)
[![Init Contracts Smoke Test](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/init-smoke-test.yml/badge.svg)](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/init-smoke-test.yml)
[![Changelog Release Check](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/release-check.yml/badge.svg)](https://github.com/WHEELBACK/COMEBACKHERE-contracts/actions/workflows/release-check.yml)

> **Rust & Soroban smart contracts powering the COMEBACKHERE Protocol.**

This repository contains the core on-chain components that power the **COMEBACKHERE Protocol** on the Stellar network. It manages invoice escrow, payment verification, treasury settlement, and compliance controls while providing a secure and auditable foundation for protocol operations.

---

## Overview

The smart contracts in this repository are responsible for:

* Managing invoice escrow and lifecycle
* Validating and recording payments
* Executing multi-signature treasury settlements
* Enforcing compliance through allowlists and blocklists

Built with **Rust** and **Soroban**, these contracts prioritize security, performance, and maintainability.

---

## Repository Structure

```text
contracts/
├── invoice/       # Invoice escrow state machine and payment lifecycle
├── treasury/      # 2-of-3 multisignature treasury settlement workflow
└── compliance/    # Admin-managed allowlist and blocklist controls
```

| Contract     | Purpose                                                                 |
| ------------ | ----------------------------------------------------------------------- |
| `invoice`    | Manages invoice creation, escrow state, and payment confirmation        |
| `treasury`   | Handles secure multi-signature settlement approvals (2-of-3)            |
| `compliance` | Maintains protocol allowlists and blocklists for compliance enforcement |

---

# Development

## Format the Code

Using Just:

```bash
just fmt
```

Or directly with Cargo:

```bash
cargo fmt --all
```

---

## Run the Linter

Using Just:

```bash
just lint
```

Or with Cargo:

```bash
cargo clippy -- -D warnings
```

---

## Run Tests

Using Just:

```bash
just test
```

Or with Cargo:

```bash
cargo test
```

---

## Run All Checks

Execute formatting, linting, and tests in a single command:

```bash
just check
```

---

## Mirror the Pre-commit CI Job Locally

Run the exact same command sequence, in the same order, as the [Pre-commit](.github/workflows/pre-commit.yml) CI job (`cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, `scripts/check-enum-ordering.sh`):

Using Just:

```bash
just precommit
```

Or with Make:

```bash
make precommit
```

---

## Run a Local Stellar Network

Start a local Stellar quickstart network matching the exact image, environment variables, and port used by the [Init Contracts Smoke Test](.github/workflows/init-smoke-test.yml) CI workflow, so `scripts/init-contracts.sh` behaves the same locally as it does in CI:

```bash
docker compose up
```

This starts `stellar/quickstart:latest` on `localhost:8000` with Soroban RPC enabled. Once healthy, run `scripts/init-contracts.sh` against it as usual.

---

# ABI Snapshots

Whenever contract interfaces change, regenerate the ABI metadata from the sibling **COMEBACKHERE** repository.

```bash
cd ../COMEBACKHERE
make update-abi-snapshots
```

This keeps the backend and other protocol components synchronized with the latest contract interfaces.

---

# Toolchain

The project is built using the following toolchain:

| Tool               | Version                  |
| ------------------ | ------------------------ |
| Rust               | `1.95.0`                 |
| Compilation Target | `wasm32-unknown-unknown` |
| Stellar CLI        | `22.8.2`                 |

> The Rust version is pinned in `rust-toolchain.toml` to ensure consistent builds across development environments.

---

## Verify Your Environment

Run the following script to confirm that all required tools are installed and correctly configured:

```bash
./scripts/check-tools.sh
```

---

# Contributing

Before submitting a pull request, make sure to:

* Format the code.
* Run the linter.
* Execute the full test suite.
* Regenerate ABI snapshots if contract interfaces changed.
* Ensure all checks pass successfully.

---

# License

This project is licensed under the **MIT License**.
