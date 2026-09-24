#!/bin/bash
set -e

# Tooling Version Check Script
# Verifies that the environment matches the required versions for COMEBACKHERE contracts.

# Structured log-line convention shared across scripts/*.sh: every line is
# `[UTC timestamp] [LEVEL] message`, so output stays greppable/pipeable for
# CI or monitoring rather than only ever being read live in a terminal.
log() {
    local level="$1"
    shift
    printf '[%s] [%s] %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$level" "$*"
}
log_info() { log "INFO" "$@"; }
log_error() { log "ERROR" "$@" >&2; }

# Version pins are centralized in .github/versions.env so the workflows and the
# local tooling checks can never drift apart. Fall back to literals only if the
# file is somehow absent.
VERSIONS_FILE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/.github/versions.env"
if [ -f "$VERSIONS_FILE" ]; then
    # shellcheck disable=SC1090
    set -a
    source "$VERSIONS_FILE"
    set +a
fi
REQUIRED_RUST="${RUST_VERSION:-1.95.0}"
REQUIRED_STELLAR_CLI="${STELLAR_CLI_VERSION:-22.8.2}"
TARGET="wasm32-unknown-unknown"

log_info "Checking development environment..."

# 1. Check Rust version
if ! command -v rustc &> /dev/null; then
    log_error "Rust is not installed."
    exit 1
fi

RUST_VERSION=$(rustc --version | cut -d' ' -f2)
if [ "$RUST_VERSION" != "$REQUIRED_RUST" ]; then
    log_error "Rust version $REQUIRED_RUST is required (found $RUST_VERSION)."
    log_error "Please update your toolchain in rust-toolchain.toml or run: rustup default $REQUIRED_RUST"
    exit 1
else
    log_info "Rust version: $RUST_VERSION"
fi

# 2. Check wasm32 target
if ! rustup target list --installed | grep -q "$TARGET"; then
    log_error "Rust target $TARGET is not installed."
    log_error "Run: rustup target add $TARGET"
    exit 1
else
    log_info "Rust target: $TARGET"
fi

# 3. Check Stellar CLI
if ! command -v stellar &> /dev/null; then
    log_error "stellar-cli is not installed."
    log_error "Install it via: cargo install --locked stellar-cli --version $REQUIRED_STELLAR_CLI"
    exit 1
fi

# stellar --version prints several lines (stellar-cli, then its embedded
# soroban-env / xdr versions); only the first line carries the CLI version.
STELLAR_VERSION=$(stellar --version | head -1 | awk '{print $2}' | tr -d '[:space:]')
if [ "$STELLAR_VERSION" != "$REQUIRED_STELLAR_CLI" ]; then
    log_error "stellar-cli version $REQUIRED_STELLAR_CLI is required (found $STELLAR_VERSION)."
    log_error "Update via: cargo install --locked stellar-cli --version $REQUIRED_STELLAR_CLI"
    exit 1
else
    log_info "stellar-cli version: $STELLAR_VERSION"
fi

log_info "All systems go! Your environment is ready for development."
