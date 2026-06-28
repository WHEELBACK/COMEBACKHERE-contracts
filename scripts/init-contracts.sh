#!/bin/bash
set -e

# COMEBACKHERE Contract Initialization Script
# Deploys and initializes Compliance, Invoice, and Treasury contracts on a local Soroban network.

NETWORK="local"
RPC_URL="http://localhost:8000"
NETWORK_PASSPHRASE="Standalone Network ; February 2017"

echo "Using network: $NETWORK ($RPC_URL)"

# 1. Build contracts
echo "Building contracts..."
cargo build --target wasm32-unknown-unknown --release

# 2. Setup network
echo "Ensuring network '$NETWORK' is configured..."
stellar network add --rpc-url "$RPC_URL" --network-passphrase "$NETWORK_PASSPHRASE" "$NETWORK" 2>/dev/null || true

# 3. Setup Admin Identity
echo "Ensuring admin identity exists..."
stellar keys generate --network "$NETWORK" admin 2>/dev/null || true
ADMIN_ADDRESS=$(stellar keys address admin)
echo "Admin Address: $ADMIN_ADDRESS"

# 4. Deploy and Initialize Compliance
echo "Deploying Compliance contract..."
COMPLIANCE_ID=$(stellar contract deploy \
    --wasm target/wasm32-unknown-unknown/release/compliance.wasm \
    --source admin \
    --network "$NETWORK")
echo "Compliance ID: $COMPLIANCE_ID"

echo "Initializing Compliance contract..."
stellar contract invoke \
    --id "$COMPLIANCE_ID" \
    --source admin \
    --network "$NETWORK" \
    -- initialize --admin "$ADMIN_ADDRESS"

# 5. Deploy and Initialize Invoice
echo "Deploying Invoice contract..."
INVOICE_ID=$(stellar contract deploy \
    --wasm target/wasm32-unknown-unknown/release/invoice.wasm \
    --source admin \
    --network "$NETWORK")
echo "Invoice ID: $INVOICE_ID"

echo "Initializing Invoice contract..."
stellar contract invoke \
    --id "$INVOICE_ID" \
    --source admin \
    --network "$NETWORK" \
    -- initialize --admin "$ADMIN_ADDRESS"

# 6. Deploy and Initialize Treasury
echo "Deploying Treasury contract..."
TREASURY_ID=$(stellar contract deploy \
    --wasm target/wasm32-unknown-unknown/release/treasury.wasm \
    --source admin \
    --network "$NETWORK")
echo "Treasury ID: $TREASURY_ID"

echo "Initializing Treasury contract..."
stellar contract invoke \
    --id "$TREASURY_ID" \
    --source admin \
    --network "$NETWORK" \
    -- initialize --admin "$ADMIN_ADDRESS" --threshold 1

echo ""
echo "============================================================"
echo "Deployment Successful!"
echo "============================================================"
echo "Compliance ID: $COMPLIANCE_ID"
echo "Invoice ID:    $INVOICE_ID"
echo "Treasury ID:   $TREASURY_ID"
echo "Admin Address: $ADMIN_ADDRESS"
echo "============================================================"
