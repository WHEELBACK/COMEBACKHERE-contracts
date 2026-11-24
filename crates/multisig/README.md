# multisig

Shared multisig types and threshold helpers for COMEBACKHERE contracts
(`comebackhere-multisig`, version `0.6.0`).

This crate is a pure library — it does not expose a `#[contract]` entry point.
Treasury, settlement-workflow, and related contracts import it as a path
dependency to share data types (errors, settlement state, proposals) and
stateless helper functions without duplicating logic across contract crates.

---

## Public API

### Helper functions

#### `signer_weight`

```rust
pub fn signer_weight(env: &Env, signer: &Address) -> u32
```

Reads the approval weight assigned to `signer` from instance storage
(`DataKey::Signer`). Returns `0` when the address is not registered —
callers interpret weight `0` as "not a signer" and typically reject the
caller before accumulating any approvals.

#### `require_authorized_signer`

```rust
pub fn require_authorized_signer(env: &Env, signer: &Address)
```

Panics with `TreasuryError::UnauthorizedSigner` if `signer_weight` returns
`0`. Contracts call this at the top of every authorized-signer entrypoint
(`propose_settlement`, `approve_settlement`, `execute_settlement`, etc.) to
reject callers that are not registered before any storage reads or writes.
Does **not** call `signer.require_auth()` — the caller is expected to have
already done that via its own `require_auth()` call so the authorization
check and the weight check are explicit and separate.

#### `record_approval`

```rust
pub fn record_approval(
    env: &Env,
    approvals: &mut Vec<Address>,
    weight: &mut u32,
    signer: &Address,
)
```

Appends `signer` to `approvals` and adds `signer_weight(env, signer)` to
`*weight`. Deduplicates silently — if `signer` is already in `approvals` the
function is a no-op, so calling it twice for the same signer does not double
the weight. Also writes `DataKey::SignerLastActive(signer)` with the current
ledger timestamp, so off-chain monitors can detect inactive signers.

Typically called inside `approve_settlement` / `approve_signer_rotation` /
`vote_dispute_resolution` after auth checks pass.

#### `revoke_approval`

```rust
pub fn revoke_approval(
    env: &Env,
    approvals: &mut Vec<Address>,
    weight: &mut u32,
    signer: &Address,
) -> bool
```

Removes `signer` from `approvals` and subtracts their weight from `*weight`.
Returns `true` if the signer was present and removed, `false` if they were
not in `approvals` (the caller can map `false` to `TreasuryError::ApprovalNotFound`).
Added in `0.5.0`.

#### `approval_expiry`

```rust
pub fn approval_expiry(env: &Env, signer: &Address, ttl_seconds: u64) -> ApprovalExpiry
```

Constructs an `ApprovalExpiry` stamped with the current ledger timestamp
(`approved_at = env.ledger().timestamp()`) and an expiry of
`approved_at + ttl_seconds`. A `ttl_seconds` of `0` produces
`expires_at = 0`, which `approval_is_active` treats as "never expires".

#### `approval_is_active`

```rust
pub fn approval_is_active(env: &Env, approval: &ApprovalExpiry) -> bool
```

Returns `true` when `approval.expires_at == 0` (no expiry) or when
`env.ledger().timestamp() <= approval.expires_at`. Contracts that enforce
approval TTLs should call this before counting stale approvals toward quorum.

#### `meets_threshold`

```rust
pub fn meets_threshold(weight: u32, threshold: u32) -> bool
```

Returns `true` when `weight >= threshold`. Pure arithmetic — takes no `Env`
reference and touches no storage. The treasury contract reads the configured
threshold from `DataKey::Threshold` once and passes it here; the separation
keeps the helper testable without a full Soroban environment.

---

### Key types

| Type | Description |
|---|---|
| `TreasuryError` | Append-only `#[contracterror]` enum, discriminants 1–47. Never renumber variants; see `scripts/check-enum-ordering.sh`. |
| `Settlement` | A pending payment proposal: amount, merchant, approvals, status, execution deadline, and token. |
| `Dispute` | A raised dispute on a settlement: claimant, counterparty, resolution approvals, and split ratio. |
| `SignerRotationProposal` | A proposal to replace one signer with another; captures old signer's weight at proposal time. |
| `SignerChangeProposal` | A timelocked admin change: `SetSigner`, `RemoveSigner`, or `UpdateThreshold`. |
| `ApprovalExpiry` | Metadata for a time-bounded approval: `signer`, `approved_at`, `expires_at`. |
| `MaybeAddress` | `Option<Address>` shim compatible with `#[contracttype]`. |
| `DataKey` | Unified storage key enum shared across treasury state. Append-only. |
| `SettlementStatus` | `Pending` → `Executed` / `Cancelled` / `Expired` / `OnHold` / partial variants. |
| `DisputeStatus` | `Raised` → `ResolvedClaimant` / `ResolvedCounterparty` / `ResolvedSplit` / `Expired`. |
| `RotationStatus` | `Pending` → `Executed` / `Cancelled`. |
| `SignerChangeStatus` | `Pending` → `Executed` / `Cancelled`. |
| `SettlementHoldReason` | `None`, `ComplianceReview`, `FraudCheck`, `KycPending`, `AdminHold`. |

---

## Short usage example

The snippet below mirrors the core of `approve_settlement` in
`contracts/treasury/src/settlements.rs`:

```rust
use multisig::{
    meets_threshold, record_approval, require_authorized_signer,
    signer_weight, DataKey, Settlement, TreasuryError,
};
use soroban_sdk::{Env, Address};

fn approve(env: &Env, signer: &Address, settlement: &mut Settlement) -> Result<(), TreasuryError> {
    // 1. Confirm the signer is registered (non-zero weight).
    require_authorized_signer(env, signer);

    // 2. Read the threshold once from storage.
    let threshold: u32 = env
        .storage()
        .instance()
        .get(&DataKey::Threshold)
        .ok_or(TreasuryError::ThresholdNotConfigured)?;

    // 3. Accumulate the signer's weight (deduplicates automatically).
    record_approval(env, &mut settlement.approvals, &mut settlement.approval_weight, signer);

    // 4. Check whether the settlement can now be executed.
    if meets_threshold(settlement.approval_weight, threshold) {
        // … proceed to execution
    }

    Ok(())
}
```

---

## Versioning

Enum variants and struct fields are **append-only**. Adding a variant to
`TreasuryError` or a key to `DataKey` is allowed; removing or renumbering
existing ones is not, because discriminants are stored on-chain and decoded
by off-chain systems. The CI job `scripts/check-enum-ordering.sh` enforces
this constraint on every pull request.

The treasury tests pin the expected version in
`contracts/treasury/tests/multisig_version_lock_test.rs` — bump
`EXPECTED_MULTISIG_VERSION` there when the `Cargo.toml` version changes.

---

## Feature flags

| Flag | Effect |
|---|---|
| `testutils` | Enables `soroban-sdk/testutils`, required to construct a real `Env` in unit tests. Do **not** enable this flag in contract dependencies — `soroban-sdk/testutils` is incompatible with the `wasm32-unknown-unknown` target. |
