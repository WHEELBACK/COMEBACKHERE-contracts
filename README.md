# COMEBACKHERE Contracts

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
| Stellar CLI        | `20.0.0`                 |

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
