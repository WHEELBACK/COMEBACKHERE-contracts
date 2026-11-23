# compliance-client

Re-exports the auto-generated Soroban cross-contract client for the
`Compliance` contract (`ComplianceContractClient`, aliased as `ComplianceClient`),
so other contracts (e.g. `Treasury`, via `SettlementWorkflow`) can call
`is_allowed` without duplicating the ABI binding boilerplate.

## Cross-contract call failure modes

Soroban cross-contract calls have no network-style timeout or retry semantics —
a call either completes synchronously within the current transaction or the
whole invocation aborts. When calling `is_allowed` through this client, callers
should be aware of:

- **Compliance contract not deployed / wrong contract ID** — the call fails
  immediately and the calling transaction aborts, rolling back any state
  changes made earlier in the same transaction.
- **Compliance contract paused** — `is_allowed` does not check the contract's
  `Paused` flag (only administrative mutations like `allow_address` /
  `block_address` do), so it still returns its normal `bool` result while paused.
- **Compliance contract panics** — the panic propagates through the call
  boundary and aborts the calling transaction, identical to a missing-contract
  failure. There is no automatic retry; a fresh transaction must be resubmitted
  by the off-chain orchestrator after the underlying issue is resolved.

See `ARCHITECTURE.md` at the repo root for the full cross-contract call map and
failure-mode documentation.
