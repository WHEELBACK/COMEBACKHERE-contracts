# COMEBACKHERE-contracts task runner

# Compile all contracts
build:
    cargo build

# Run all tests
test:
    cargo test

# Format all code
fmt:
    cargo fmt --all

# Run clippy lints
lint:
    cargo clippy -- -D warnings

# Check dependencies for vulnerabilities and license issues
deny:
    cargo deny check

# Audit dependencies for known security vulnerabilities
audit:
    cargo audit

# Run format and lint checks (for CI)
check: fmt lint test deny
    @echo "✓ All checks passed"

# Run all quality gates before pushing
check-all: fmt lint test audit
    @echo "✓ All quality gates passed"

# Deploy all three contracts to testnet.
#
# Required env vars:
#   STELLAR_ACCOUNT   — signing identity (key name or secret key), e.g. "alice"
#   STELLAR_NETWORK   — network alias configured in the Stellar CLI, e.g. "testnet"
#
# Optional env vars (override when using a custom RPC):
#   STELLAR_RPC_URL          — RPC endpoint (defaults to the CLI network alias)
#   STELLAR_NETWORK_PASSPHRASE — network passphrase (defaults to the CLI network alias)
#
# Example:
#   STELLAR_ACCOUNT=alice STELLAR_NETWORK=testnet just deploy
deploy:
    #!/usr/bin/env bash
    set -euo pipefail
    : "${STELLAR_ACCOUNT:?STELLAR_ACCOUNT must be set}"
    : "${STELLAR_NETWORK:?STELLAR_NETWORK must be set}"

    cargo build --target wasm32-unknown-unknown --release

    WASM_DIR="target/wasm32-unknown-unknown/release"

    deploy_contract() {
        local name="$1"
        local id
        id=$(stellar contract deploy \
            --wasm "$WASM_DIR/${name}.wasm" \
            --source "$STELLAR_ACCOUNT" \
            --network "$STELLAR_NETWORK")
        echo "$name contract ID: $id"
    }

    deploy_contract compliance
    deploy_contract invoice
    deploy_contract treasury

# Default target
default: check
