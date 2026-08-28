#!/bin/bash
set -e

# COMEBACKHERE Contract Initialization Script
# Deploys and initializes Compliance, Invoice, Treasury, and Settlement Workflow contracts on a local Soroban network.

NETWORK="local"
RPC_URL="http://localhost:8000"
NETWORK_PASSPHRASE="Standalone Network ; February 2017"

echo "Using network: $NETWORK ($RPC_URL)"

# 1. Build contracts
# Scoped to the packages actually deployed below (not `--workspace` or an
# unscoped `cargo build`): the workspace also contains comebackhere-tests,
# whose `default = ["testutils"]` feature enables soroban-sdk/testutils,
# which soroban-sdk hard-disables on the wasm32 target.
echo "Building contracts..."
cargo build --target wasm32-unknown-unknown --release \
    -p comebackhere-compliance \
    -p comebackhere-invoice \
    -p comebackhere-treasury \
    -p comebackhere-settlement-workflow

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

# 7. Deploy and Initialize Settlement Workflow
echo "Deploying Settlement Workflow contract..."
SETTLEMENT_WORKFLOW_ID=$(stellar contract deploy \
    --wasm target/wasm32-unknown-unknown/release/settlement_workflow.wasm \
    --source admin \
    --network "$NETWORK")
echo "Settlement Workflow ID: $SETTLEMENT_WORKFLOW_ID"

echo "Initializing Settlement Workflow contract..."
stellar contract invoke \
    --id "$SETTLEMENT_WORKFLOW_ID" \
    --source admin \
    --network "$NETWORK" \
    -- initialize --compliance_id "$COMPLIANCE_ID" --treasury_id "$TREASURY_ID"

echo ""
echo "============================================================"
echo "Deployment Successful!"
echo "============================================================"
echo "Compliance ID:          $COMPLIANCE_ID"
echo "Invoice ID:             $INVOICE_ID"
echo "Treasury ID:            $TREASURY_ID"
echo "Settlement Workflow ID: $SETTLEMENT_WORKFLOW_ID"
echo "Admin Address:          $ADMIN_ADDRESS"
echo "============================================================"
