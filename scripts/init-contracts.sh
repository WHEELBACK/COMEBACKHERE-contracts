#!/bin/bash
set -e

# COMEBACKHERE Contract Initialization Script
# Deploys and initializes Compliance, Invoice, Treasury, and Settlement Workflow contracts on a local Soroban network.

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

NETWORK="local"
RPC_URL="http://localhost:8000/soroban/rpc"
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

log_info "Using network: $NETWORK ($RPC_URL)"

# 1. Build contracts
# Scoped to the packages actually deployed below (not `--workspace` or an
# unscoped `cargo build`): the workspace also contains comebackhere-tests,
# whose `default = ["testutils"]` feature enables soroban-sdk/testutils,
# which soroban-sdk hard-disables on the wasm32 target.
#
# target-cpu=mvp stops rustc emitting post-MVP instructions (e.g. bulk-memory
# `memory.copy`) that Rust >= 1.82 enables by default for wasm32 and the
# Soroban host rejects; see the wasm-opt step below for the rest.
log_info "Building contracts..."
SOROBAN_SDK_BUILD_SYSTEM_SUPPORTS_SPEC_SHAKING_V2=1 cargo build --target wasm32v1-none --release \
    -p comebackhere-compliance \
    -p comebackhere-invoice \
    -p comebackhere-treasury \
    -p comebackhere-settlement-workflow

if ! command -v wasm-opt > /dev/null 2>&1; then
    log_error "wasm-opt not found. Install binaryen (e.g. 'apt-get install binaryen' or 'brew install binaryen')."
    exit 1
fi
log_info "Optimizing contract wasm..."
for contract in compliance invoice treasury settlement_workflow; do
    wasm="target/wasm32v1-none/release/${contract}.wasm"
    wasm-opt -Oz --strip-debug --strip-dwarf --vacuum --all-features "$wasm" -o "$wasm"
done

# 2. Setup network
log_info "Ensuring network '$NETWORK' is configured..."
stellar network add --rpc-url "$RPC_URL" --network-passphrase "$NETWORK_PASSPHRASE" "$NETWORK" 2>/dev/null || true

# 3. Setup Admin Identity
log_info "Ensuring admin identity exists..."
stellar keys generate --network "$NETWORK" --fund admin 2>/dev/null || stellar keys generate admin 2>/dev/null || true
ADMIN_ADDRESS=$(stellar keys address admin)
log_info "Admin Address: $ADMIN_ADDRESS"
log_info "Ensuring admin account is funded..."
stellar keys fund admin --network "$NETWORK" 2>/dev/null || curl -sf "http://localhost:8000/friendbot?addr=${ADMIN_ADDRESS}" > /dev/null || true

# 4. Deploy and Initialize Compliance
log_info "Deploying Compliance contract..."
COMPLIANCE_ID=$(stellar contract deploy \
    --wasm target/wasm32v1-none/release/compliance.wasm \
    --source admin \
    --network "$NETWORK")
log_info "Compliance ID: $COMPLIANCE_ID"

log_info "Initializing Compliance contract..."
stellar contract invoke \
    --id "$COMPLIANCE_ID" \
    --source admin \
    --network "$NETWORK" \
    -- initialize --admin "$ADMIN_ADDRESS"

# 5. Deploy and Initialize Invoice
log_info "Deploying Invoice contract..."
INVOICE_ID=$(stellar contract deploy \
    --wasm target/wasm32v1-none/release/invoice.wasm \
    --source admin \
    --network "$NETWORK")
log_info "Invoice ID: $INVOICE_ID"

log_info "Initializing Invoice contract..."
stellar contract invoke \
    --id "$INVOICE_ID" \
    --source admin \
    --network "$NETWORK" \
    -- initialize --admin "$ADMIN_ADDRESS"

# 6. Deploy and Initialize Treasury
log_info "Deploying Treasury contract..."
TREASURY_ID=$(stellar contract deploy \
    --wasm target/wasm32v1-none/release/treasury.wasm \
    --source admin \
    --network "$NETWORK")
log_info "Treasury ID: $TREASURY_ID"

log_info "Initializing Treasury contract..."
stellar contract invoke \
    --id "$TREASURY_ID" \
    --source admin \
    --network "$NETWORK" \
    -- initialize --admin "$ADMIN_ADDRESS" --threshold 1 --signers '[]'

# 7. Deploy and Initialize Settlement Workflow (uses __constructor at deploy time)
echo "Deploying and initializing Settlement Workflow contract..."
SETTLEMENT_WORKFLOW_ID=$(stellar contract deploy \
    --wasm target/wasm32v1-none/release/settlement_workflow.wasm \
    --source admin \
    --network "$NETWORK" \
    -- \
    --admin "$ADMIN_ADDRESS" --compliance_id "$COMPLIANCE_ID" --treasury_id "$TREASURY_ID")
echo "Settlement Workflow ID: $SETTLEMENT_WORKFLOW_ID"

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
