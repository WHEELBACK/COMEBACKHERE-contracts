#!/bin/bash
set -e

# COMEBACKHERE Contract Initialization Script
# Deploys and initializes Compliance, Invoice, and Treasury contracts on a local Soroban network.

NETWORK="local"
RPC_URL="http://localhost:8000"
NETWORK_PASSPHRASE="Standalone Network ; February 2017"
DRY_RUN=false

# Parse flags
while [[ $# -gt 0 ]]; do
    case $1 in
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        *)
            echo "Unknown flag: $1"
            echo "Usage: $0 [--dry-run]"
            exit 1
            ;;
    esac
done

if [ "$DRY_RUN" = true ]; then
    echo "DRY RUN MODE: Commands will be printed but not executed"
    echo ""
fi

echo "Using network: $NETWORK ($RPC_URL)"

# 1. Build contracts
# Scoped to the packages actually deployed below (not `--workspace` or an
# unscoped `cargo build`): the workspace also contains comebackhere-tests,
# whose `default = ["testutils"]` feature enables soroban-sdk/testutils,
# which soroban-sdk hard-disables on the wasm32 target.
echo "Building contracts..."
if [ "$DRY_RUN" = true ]; then
    echo "cargo build --target wasm32-unknown-unknown --release \\"
    echo "    -p comebackhere-compliance \\"
    echo "    -p comebackhere-invoice \\"
    echo "    -p comebackhere-treasury"
else
    cargo build --target wasm32-unknown-unknown --release \
        -p comebackhere-compliance \
        -p comebackhere-invoice \
        -p comebackhere-treasury
fi

# 2. Setup network
echo "Ensuring network '$NETWORK' is configured..."
if [ "$DRY_RUN" = true ]; then
    echo "stellar network add --rpc-url \"$RPC_URL\" --network-passphrase \"$NETWORK_PASSPHRASE\" \"$NETWORK\""
else
    stellar network add --rpc-url "$RPC_URL" --network-passphrase "$NETWORK_PASSPHRASE" "$NETWORK" 2>/dev/null || true
fi

# 3. Setup Admin Identity
echo "Ensuring admin identity exists..."
if [ "$DRY_RUN" = true ]; then
    echo "stellar keys generate --network \"$NETWORK\" admin"
    ADMIN_ADDRESS="<admin-address-placeholder>"
    echo "Admin Address: $ADMIN_ADDRESS"
else
    stellar keys generate --network "$NETWORK" admin 2>/dev/null || true
    ADMIN_ADDRESS=$(stellar keys address admin)
    echo "Admin Address: $ADMIN_ADDRESS"
fi

# 4. Deploy and Initialize Compliance
echo "Deploying Compliance contract..."
if [ "$DRY_RUN" = true ]; then
    echo "stellar contract deploy \\"
    echo "    --wasm target/wasm32-unknown-unknown/release/compliance.wasm \\"
    echo "    --source admin \\"
    echo "    --network \"$NETWORK\""
    COMPLIANCE_ID="<compliance-contract-id-placeholder>"
else
    COMPLIANCE_ID=$(stellar contract deploy \
        --wasm target/wasm32-unknown-unknown/release/compliance.wasm \
        --source admin \
        --network "$NETWORK")
fi
echo "Compliance ID: $COMPLIANCE_ID"

echo "Initializing Compliance contract..."
if [ "$DRY_RUN" = true ]; then
    echo "stellar contract invoke \\"
    echo "    --id \"$COMPLIANCE_ID\" \\"
    echo "    --source admin \\"
    echo "    --network \"$NETWORK\" \\"
    echo "    -- initialize --admin \"$ADMIN_ADDRESS\""
else
    stellar contract invoke \
        --id "$COMPLIANCE_ID" \
        --source admin \
        --network "$NETWORK" \
        -- initialize --admin "$ADMIN_ADDRESS"
fi

# 5. Deploy and Initialize Invoice
echo "Deploying Invoice contract..."
if [ "$DRY_RUN" = true ]; then
    echo "stellar contract deploy \\"
    echo "    --wasm target/wasm32-unknown-unknown/release/invoice.wasm \\"
    echo "    --source admin \\"
    echo "    --network \"$NETWORK\""
    INVOICE_ID="<invoice-contract-id-placeholder>"
else
    INVOICE_ID=$(stellar contract deploy \
        --wasm target/wasm32-unknown-unknown/release/invoice.wasm \
        --source admin \
        --network "$NETWORK")
fi
echo "Invoice ID: $INVOICE_ID"

echo "Initializing Invoice contract..."
if [ "$DRY_RUN" = true ]; then
    echo "stellar contract invoke \\"
    echo "    --id \"$INVOICE_ID\" \\"
    echo "    --source admin \\"
    echo "    --network \"$NETWORK\" \\"
    echo "    -- initialize --admin \"$ADMIN_ADDRESS\""
else
    stellar contract invoke \
        --id "$INVOICE_ID" \
        --source admin \
        --network "$NETWORK" \
        -- initialize --admin "$ADMIN_ADDRESS"
fi

# 6. Deploy and Initialize Treasury
echo "Deploying Treasury contract..."
if [ "$DRY_RUN" = true ]; then
    echo "stellar contract deploy \\"
    echo "    --wasm target/wasm32-unknown-unknown/release/treasury.wasm \\"
    echo "    --source admin \\"
    echo "    --network \"$NETWORK\""
    TREASURY_ID="<treasury-contract-id-placeholder>"
else
    TREASURY_ID=$(stellar contract deploy \
        --wasm target/wasm32-unknown-unknown/release/treasury.wasm \
        --source admin \
        --network "$NETWORK")
fi
echo "Treasury ID: $TREASURY_ID"

echo "Initializing Treasury contract..."
if [ "$DRY_RUN" = true ]; then
    echo "stellar contract invoke \\"
    echo "    --id \"$TREASURY_ID\" \\"
    echo "    --source admin \\"
    echo "    --network \"$NETWORK\" \\"
    echo "    -- initialize --admin \"$ADMIN_ADDRESS\" --threshold 1"
else
    stellar contract invoke \
        --id "$TREASURY_ID" \
        --source admin \
        --network "$NETWORK" \
        -- initialize --admin "$ADMIN_ADDRESS" --threshold 1
fi

echo ""
echo "============================================================"
echo "Deployment Successful!"
echo "============================================================"
echo "Compliance ID: $COMPLIANCE_ID"
echo "Invoice ID:    $INVOICE_ID"
echo "Treasury ID:   $TREASURY_ID"
echo "Admin Address: $ADMIN_ADDRESS"
echo "============================================================"
