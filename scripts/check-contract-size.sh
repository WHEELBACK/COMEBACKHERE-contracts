#!/usr/bin/env bash
#
# #435: Local reproduction of the Contract Size CI check
#
# Builds every contract under contracts/ to wasm32-unknown-unknown in release
# mode, optimizes the resulting binaries the same way `stellar contract
# optimize` does before deployment, and compares each one against
# MAX_CONTRACT_SIZE.
#
# This is the single implementation of that check: .github/workflows/
# contract-size.yml calls this script, so CI and local runs can never drift
# apart.
#
# Usage:
#   ./scripts/check-contract-size.sh
#
# Environment:
#   MAX_CONTRACT_SIZE   size ceiling in bytes (default 65536, i.e. 64 KB)
#   GITHUB_STEP_SUMMARY when set, a markdown table is appended to that file
#
# Exit codes:
#   0 - every contract is within the size ceiling
#   1 - one or more contracts exceed it

set -euo pipefail

MAX_CONTRACT_SIZE="${MAX_CONTRACT_SIZE:-65536}"
WASM_DIR="target/wasm32-unknown-unknown/release"

echo "=== Building contracts ==="
for contract in contracts/*/; do
    if [ -f "$contract/Cargo.toml" ]; then
        name=$(basename "$contract")
        echo "--- Building $name ---"
        cargo build --package "comebackhere-$name" --target wasm32-unknown-unknown --release 2>&1
    fi
done

# Mirrors the size reduction `stellar contract optimize` performs before
# deployment, so the measured size matches what actually ships. wasm-opt only
# touches code/data, not the custom contractspecv0/contractmetav0/
# contractenvmetav0 sections that encode the contract's on-chain ABI.
# --all-features: the apt-packaged wasm-opt doesn't read the module's
# target-features custom section, so it must be told about every feature LLVM's
# wasm32 backend may emit (sign-ext, bulk-memory, etc.) rather than enabling
# them one by one.
if command -v wasm-opt > /dev/null 2>&1; then
    echo "=== Optimizing contract WASM ==="
    for wasm in "$WASM_DIR"/*.wasm; do
        wasm-opt -Oz --strip-debug --strip-dwarf --vacuum --all-features "$wasm" -o "$wasm.opt"
        mv "$wasm.opt" "$wasm"
    done
else
    echo "WARNING: wasm-opt not found, reporting unoptimized sizes."
    echo "         Install binaryen to reproduce the CI numbers exactly."
fi

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    echo "### Contract WASM sizes" >> "$GITHUB_STEP_SUMMARY"
    echo '| Contract | Size (bytes) | Status |' >> "$GITHUB_STEP_SUMMARY"
    echo '|----------|-------------|--------|' >> "$GITHUB_STEP_SUMMARY"
fi

EXIT_CODE=0
for wasm in "$WASM_DIR"/*.wasm; do
    name=$(basename "$wasm" .wasm)
    size=$(stat --format=%s "$wasm")
    if [ "$size" -gt "$MAX_CONTRACT_SIZE" ]; then
        status=":x: Exceeds ${MAX_CONTRACT_SIZE} bytes"
        EXIT_CODE=1
    else
        status=":white_check_mark: OK"
    fi
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
        echo "| $name | $size | $status |" >> "$GITHUB_STEP_SUMMARY"
    fi
    echo "$name: ${size} bytes"
done

exit $EXIT_CODE
