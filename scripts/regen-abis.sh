#!/usr/bin/env bash
# Regenerates every ABI snapshot under abis/ from the corresponding contract's
# source, in one pass, so no snapshot has to be kept in sync by hand.
#
# Function extraction only counts `pub fn`s declared directly inside a
# `#[contractimpl] impl ... { }` block - the contract's actual public
# entrypoints - rather than every `pub fn` in the crate, so internal helpers
# (e.g. invoice's validation.rs `require_*` functions) don't leak into the ABI.
#
# Event extraction scans every *.rs file under a contract's src/ tree rather
# than assuming events live in one centralized events.rs - contracts like
# treasury emit events from several files (settlements.rs, disputes.rs,
# holds.rs, signers.rs) with no single events module.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

CONTRACTS=(invoice treasury compliance settlement-workflow)

extract_functions() {
  find "$1" -name '*.rs' -print0 | xargs -0 awk '
    /#\[contractimpl\]/ { armed=1; next }
    armed && /impl[ \t]/ { in_impl=1; armed=0; depth=0 }
    in_impl {
      line=$0
      opens=gsub(/{/,"{",line)
      closes=gsub(/}/,"}",line)
      before=depth
      depth+=opens-closes
      if (before==1 && $0 ~ /^[ \t]*pub fn /) print
      if (depth<=0) in_impl=0
    }
  ' | sed 's/.*pub fn \([a-z_][a-z0-9_]*\).*/\1/' | LC_ALL=C sort -u
}

extract_events() {
  { grep -rh 'Symbol::new(env, ' "$1" || true; } \
    | { grep -v '#\[' || true; } \
    | sed 's/.*Symbol::new(env, "\([^"]*\)").*/\1/' \
    | LC_ALL=C sort -u
}

for contract in "${CONTRACTS[@]}"; do
  src_dir="contracts/${contract}/src"
  if [ ! -d "$src_dir" ]; then
    echo "::warning::skipping ${contract}: ${src_dir} does not exist"
    continue
  fi

  functions_json=$(extract_functions "$src_dir" | jq -R -s 'split("\n") | map(select(length > 0))')
  events_json=$(extract_events "$src_dir" | jq -R -s 'split("\n") | map(select(length > 0))')

  jq -n --argjson functions "$functions_json" --argjson events "$events_json" \
    '{functions: $functions, events: $events}' > "abis/${contract}.json"

  echo "regenerated abis/${contract}.json"
done
