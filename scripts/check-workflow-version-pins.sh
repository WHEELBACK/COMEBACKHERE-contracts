#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSIONS_FILE="$ROOT_DIR/.github/versions.env"
WORKFLOWS_DIR="$ROOT_DIR/.github/workflows"

if [[ ! -f "$VERSIONS_FILE" ]]; then
  echo "Missing central version pins file: $VERSIONS_FILE" >&2
  exit 1
fi

# shellcheck disable=SC1090
set -a
source "$VERSIONS_FILE"
set +a

if [[ -z "${RUST_VERSION:-}" || -z "${STELLAR_CLI_VERSION:-}" ]]; then
  echo "Central version pins file is missing RUST_VERSION or STELLAR_CLI_VERSION" >&2
  exit 1
fi

matches=$(grep -RInE '1\.95\.0|22\.8\.2' "$WORKFLOWS_DIR"/*.yml 2>/dev/null || true)
if [[ -z "$matches" ]]; then
  echo "Workflow version pins are centralized in .github/versions.env."
  exit 0
fi

violations=()
while IFS= read -r line; do
  file="${line%%:*}"
  rest="${line#*: }"
  if [[ "$rest" =~ ^[[:space:]]*# ]]; then
    continue
  fi
  if [[ "$rest" =~ \.github/versions\.env|env\.RUST_VERSION|env\.STELLAR_CLI_VERSION|\$\{\{[[:space:]]*env\.[A-Z_]+[[:space:]]*\}\}|\$RUST_VERSION|\$STELLAR_CLI_VERSION ]]; then
    continue
  fi
  violations+=("$line")
done <<< "$matches"

if (( ${#violations[@]} > 0 )); then
  echo "Hardcoded workflow version pins detected. Use .github/versions.env instead:" >&2
  printf '%s\n' "${violations[@]}" >&2
  exit 1
fi

echo "Workflow version pins are centralized in .github/versions.env."
