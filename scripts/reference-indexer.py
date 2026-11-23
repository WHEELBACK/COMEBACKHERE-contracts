#!/usr/bin/env python3
"""Minimal reference event indexer for the COMEBACKHERE contracts.

Polls Soroban RPC's getEvents endpoint for contract events emitted by the
compliance, invoice, and treasury contracts, decodes them generically (no
per-event struct schema needed -- Soroban structs serialize as SCMaps keyed
by their Rust field names, which is self-describing), and prints one JSON
line per event to stdout.

This exists as a working, minimal starting point for real
indexing/monitoring integrations, and as a validation check on this
protocol's event schema: if this script can correctly decode every emitted
event, the schema is genuinely sufficient to build a real consumer against.

Dependency: pip install stellar-sdk

Usage:
    python3 scripts/reference-indexer.py \\
        --rpc-url http://localhost:8000/soroban/rpc \\
        --contract-id CCOMPLIANCE... --contract-id CINVOICE... --contract-id CTREASURY...

    # Start from a specific ledger instead of the latest:
    python3 scripts/reference-indexer.py --contract-id C... --start-ledger 12345

    # Print a fixed number of events then exit, instead of polling forever:
    python3 scripts/reference-indexer.py --contract-id C... --once
"""
from __future__ import annotations

import argparse
import json
import sys
import time
from typing import Any

try:
    from stellar_sdk import SorobanServer, scval, xdr as stellar_xdr
    from stellar_sdk.soroban_rpc import EventFilter, EventFilterType
except ImportError:
    print(
        "error: the 'stellar-sdk' package is required.\n"
        "       install it with: pip install stellar-sdk",
        file=sys.stderr,
    )
    sys.exit(1)

DEFAULT_RPC_URL = "http://localhost:8000/soroban/rpc"
DEFAULT_POLL_INTERVAL_SECONDS = 5


def decode_scval_b64(value_xdr_b64: str) -> Any:
    """Decode a base64 XDR SCVal into a plain-Python value.

    `scval.to_native` recurses through vecs/maps/structs/options on its own;
    struct fields come back as dict keys because #[contracttype] structs
    serialize as an SCMap keyed by Symbol field names.
    """
    scv = stellar_xdr.SCVal.from_xdr(value_xdr_b64)
    return scval.to_native(scv)


def decode_event(event) -> dict:
    topics = [decode_scval_b64(t) for t in event.topic]
    data = decode_scval_b64(event.value)
    event_name = topics[0] if topics else None
    return {
        "ledger": event.ledger,
        "ledger_closed_at": event.ledger_closed_at,
        "contract_id": event.contract_id,
        "event_id": event.id,
        "event_name": event_name,
        "topics": topics[1:],
        "data": data,
        "in_successful_contract_call": event.in_successful_contract_call,
    }


def run(rpc_url: str, contract_ids: list[str], start_ledger: int | None,
        poll_interval: float, once: bool) -> None:
    server = SorobanServer(rpc_url)
    event_filter = EventFilter(event_type=EventFilterType.CONTRACT, contract_ids=contract_ids)

    if start_ledger is None:
        start_ledger = server.get_latest_ledger().sequence

    cursor = None
    while True:
        kwargs = {"filters": [event_filter], "limit": 100}
        if cursor is None:
            kwargs["start_ledger"] = start_ledger
        else:
            kwargs["cursor"] = cursor

        response = server.get_events(**kwargs)

        for event in response.events:
            print(json.dumps(decode_event(event), default=str))
            sys.stdout.flush()
            cursor = event.paging_token

        if once:
            return
        if not response.events:
            time.sleep(poll_interval)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--rpc-url", default=DEFAULT_RPC_URL, help="Soroban RPC endpoint")
    parser.add_argument(
        "--contract-id",
        dest="contract_ids",
        action="append",
        required=True,
        help="Contract ID to watch (repeatable, e.g. one per compliance/invoice/treasury deployment)",
    )
    parser.add_argument("--start-ledger", type=int, default=None, help="Ledger to start from (default: latest)")
    parser.add_argument(
        "--poll-interval",
        type=float,
        default=DEFAULT_POLL_INTERVAL_SECONDS,
        help="Seconds to wait between polls when caught up (default: %(default)s)",
    )
    parser.add_argument("--once", action="store_true", help="Fetch one batch of events and exit instead of polling")
    args = parser.parse_args()

    run(args.rpc_url, args.contract_ids, args.start_ledger, args.poll_interval, args.once)


if __name__ == "__main__":
    main()
