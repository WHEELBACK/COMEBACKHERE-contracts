#!/usr/bin/env bash
# check-wasm-size.sh — treasury wasm size regression guard.
#
# Usage:
#   ./scripts/check-wasm-size.sh [--update] [path/to/treasury.wasm]
#
# Options:
#   --update   Write the current wasm size as the new baseline and exit 0.
#
# The script reads a numeric byte-count baseline from
# scripts/treasury-wasm-size.baseline and fails if the supplied (or default)
# wasm is more than 2 048 bytes (2 KB) larger than that baseline.
#
# Exit codes:
#   0 — within budget
#   1 — exceeds baseline + 2 KB margin

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASELINE_FILE="${SCRIPT_DIR}/treasury-wasm-size.baseline"
MARGIN=2048

# ---- Parse arguments -------------------------------------------------------
UPDATE=0
WASM_PATH=""

for arg in "$@"; do
  case "$arg" in
    --update)
      UPDATE=1
      ;;
    *)
      WASM_PATH="$arg"
      ;;
  esac
done

# Default wasm path if not provided
if [ -z "$WASM_PATH" ]; then
  WASM_PATH="target/wasm32-unknown-unknown/release/treasury.wasm"
fi

# ---- Validate wasm file exists ---------------------------------------------
if [ ! -f "$WASM_PATH" ]; then
  echo "ERROR: wasm file not found: $WASM_PATH" >&2
  exit 1
fi

CURRENT_SIZE=$(stat --format=%s "$WASM_PATH")

# ---- --update mode: write new baseline and exit ----------------------------
if [ "$UPDATE" -eq 1 ]; then
  echo "$CURRENT_SIZE" > "$BASELINE_FILE"
  echo "Baseline updated: $CURRENT_SIZE bytes written to $BASELINE_FILE"
  exit 0
fi

# ---- Read baseline ---------------------------------------------------------
if [ ! -f "$BASELINE_FILE" ]; then
  echo "ERROR: baseline file not found: $BASELINE_FILE" >&2
  echo "Run with --update to create it." >&2
  exit 1
fi

BASELINE=$(tr -d '[:space:]' < "$BASELINE_FILE")

if ! [[ "$BASELINE" =~ ^[0-9]+$ ]]; then
  echo "ERROR: baseline file does not contain a valid byte count: '$BASELINE'" >&2
  exit 1
fi

BUDGET=$(( BASELINE + MARGIN ))

# ---- Report ----------------------------------------------------------------
echo "treasury.wasm size : ${CURRENT_SIZE} bytes"
echo "Baseline           : ${BASELINE} bytes"
echo "Allowed budget     : ${BUDGET} bytes  (baseline + ${MARGIN} margin)"

if [ "$CURRENT_SIZE" -gt "$BUDGET" ]; then
  echo ""
  echo -e "\033[0;31mFAIL\033[0m — treasury.wasm (${CURRENT_SIZE} B) exceeds baseline + 2 KB margin (${BUDGET} B)."
  echo "If this growth is intentional, run: bash scripts/check-wasm-size.sh --update"
  exit 1
else
  echo ""
  echo -e "\033[0;32mOK\033[0m — treasury.wasm is within the size budget."
  exit 0
fi
