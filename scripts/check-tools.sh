#!/bin/bash
set -e

# Tooling Version Check Script
# Verifies that the environment matches the required versions for COMEBACKHERE contracts.

REQUIRED_RUST="1.95.0"
REQUIRED_STELLAR_CLI="20.0.0"
TARGET="wasm32-unknown-unknown"

echo "Checking development environment..."

# 1. Check Rust version
if ! command -v rustc &> /dev/null; then
    echo "❌ Error: Rust is not installed."
    exit 1
fi

RUST_VERSION=$(rustc --version | cut -d' ' -f2)
if [ "$RUST_VERSION" != "$REQUIRED_RUST" ]; then
    echo "❌ Error: Rust version $REQUIRED_RUST is required (found $RUST_VERSION)."
    echo "   Please update your toolchain in rust-toolchain.toml or run: rustup default $REQUIRED_RUST"
    exit 1
else
    echo "✅ Rust version: $RUST_VERSION"
fi

# 2. Check wasm32 target
if ! rustup target list --installed | grep -q "$TARGET"; then
    echo "❌ Error: Rust target $TARGET is not installed."
    echo "   Run: rustup target add $TARGET"
    exit 1
else
    echo "✅ Rust target: $TARGET"
fi

# 3. Check Stellar CLI
if ! command -v stellar &> /dev/null; then
    echo "❌ Error: stellar-cli is not installed."
    echo "   Install it via: cargo install --locked stellar-cli --version $REQUIRED_STELLAR_CLI"
    exit 1
fi

# stellar --version output format: "stellar 20.0.0 (build-date)"
STELLAR_VERSION=$(stellar --version | awk '{print $2}')
if [[ "$STELLAR_VERSION" != "$REQUIRED_STELLAR_CLI"* ]]; then
    echo "❌ Error: stellar-cli version $REQUIRED_STELLAR_CLI is required (found $STELLAR_VERSION)."
    echo "   Update via: cargo install --locked stellar-cli --version $REQUIRED_STELLAR_CLI"
    exit 1
else
    echo "✅ stellar-cli version: $STELLAR_VERSION"
fi

echo ""
echo "All systems go! Your environment is ready for development."
