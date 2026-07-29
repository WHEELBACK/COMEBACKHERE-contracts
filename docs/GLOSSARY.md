# Protocol Glossary

Canonical definitions for terms used across `ARCHITECTURE.md`, `SECURITY.md`, and the contract READMEs. Definitions are grounded in this repository's actual implementation, not generic DeFi terminology.

---

## A

### Admin

A privileged `Address` stored under `DataKey::Admin` (instance storage) in each contract. The admin is set once during `initialize` and is the only caller permitted to perform privileged mutations — e.g. `mark_paid`, `release_escrow`, `set_signer`, `pause`. All three contracts (invoice, treasury, compliance) use the same pattern: `require_auth` on the provided address plus a storage equality check against the stored `Admin` key.

The compliance and invoice contracts additionally support a **two-step admin transfer** via `transfer_admin` / `accept_admin`. The candidate is staged under `DataKey::PendingAdmin` and only promoted to `Admin` after the new address calls `accept_admin` and authenticates. This prevents accidental admin lock-out from a mis-typed address.

### Allowlist (compliance)

The set of addresses for which `Compliance::is_allowed` returns `true`. Each address is tracked independently under `DataKey::Allowed(Address)` (persistent storage). An address on the allowlist may additionally have a time-bound expiry recorded under `DataKey::AllowedUntil(Address)`. An address that is also `Blocked` is **not** considered allowed — the block flag takes precedence regardless of allow state. See also: [Blocked](#blocked), [is\_allowed precedence](#is_allowed-precedence).

### Approval weight

The numeric sum of signer weights that have approved a given settlement or dispute resolution proposal. Tracked in `Settlement.approval_weight` (treasury, persistent storage). Each call to `record_approval` (in `crates/multisig`) adds the signer's weight to the running total only if that signer has not already approved. Execution is gated on `approval_weight >= threshold`. See also: [Quorum](#quorum), [Threshold](#threshold), [Signer weight](#signer-weight).

---

## B

### Batch entrypoint

An entrypoint that accepts a `Vec` of inputs and processes them in a single transaction. Examples: `batch_create_invoice`, `batch_expire` (invoice), `batch_approve_settlements`, `batch_cancel_settlements` (treasury), and bulk allow/block operations (compliance). All batch entrypoints in this repo enforce a `MAX_BATCH_SIZE` cap (currently **50** for most operations, **100** for `batch_expire`) to bound per-invocation storage writes and compute budget.

### Blocked

An address that has been explicitly denied by the compliance contract via `block_address`. Stored under `DataKey::Blocked(Address)`. A blocked address always causes `is_allowed` to return `false`, overriding any allow-list entry. `is_blocked` returns the raw flag and does not factor in expiry timestamps. See also: [is\_allowed precedence](#is_allowed-precedence).

---

## C

### Circuit-breaker (pause)

A boolean flag stored under `DataKey::Paused` (instance storage) in each contract. When `true`, any entrypoint that calls `require_not_paused` reverts immediately with `ContractPaused`. Admin-only `pause` and `unpause` entrypoints control the flag. In the compliance contract, `block_address` is intentionally exempt from the pause check — blocking an address must always be possible even while the contract is paused for other operations.

### Compliance gate

The off-chain (or intermediary-contract) step in the settlement workflow where `Compliance::is_allowed(merchant)` is called before `Treasury::execute_settlement`. The treasury contract itself does not call compliance directly; the check is performed by the `SettlementWorkflow` coordinator. If the compliance contract is paused, `is_allowed` still executes normally — the pause only blocks administrative list mutations. See `ARCHITECTURE.md → Cross-Contract Compliance Call Failure Modes` for what happens when the compliance contract is missing or panics.

### Creation cooldown

An admin-tunable minimum interval (in seconds) enforced between successive `create_invoice` calls by the same merchant. Stored under `DataKey::CreationCooldown` (instance, invoice contract). The timestamp of the merchant's last successful creation is stored under `DataKey::LastCreatedAt(Address)`. If a merchant calls `create_invoice` before the cooldown has elapsed, the call reverts with `CooldownActive` (error code 21).

---

## D

### Dispute

A formal on-chain challenge raised against a specific settlement via `Treasury::raise_dispute`. Stored under `DataKey::Dispute(u64)` (persistent storage) as a `Dispute` struct containing: dispute ID, linked `settlement_id`, `claimant`, `counterparty`, disputed `amount`, current `DisputeStatus`, accumulated `resolution_approvals` and `resolution_weight`, direction flag `resolution_for_claimant`, and `dispute_expires_at` (a UNIX timestamp after which `expire_dispute` may be called).

Raising a dispute automatically transitions the linked settlement from `Pending` to `OnHold`. The hold is released back to `Pending` once all open disputes for that settlement are resolved or expired. Disputes are resolved either by admin via `resolve_dispute` or by weighted signer vote via `vote_dispute_resolution` (auto-resolves when cumulative vote weight meets the treasury threshold).

### Dispute status

The lifecycle states of a `Dispute`:

| Status | Meaning |
|---|---|
| `Raised` | Dispute is open; linked settlement is on hold. |
| `ResolvedClaimant` | Resolved in favour of the claimant. |
| `ResolvedCounterparty` | Resolved in favour of the counterparty. |
| `Expired` | Dispute deadline elapsed; `expire_dispute` was called; linked settlement released. |

---

## E

### Escrow

The implied custody of invoice funds between `mark_paid` and `release_escrow`. There is no explicit token lock in the invoice contract; "escrow" describes the logical state where an invoice is `Paid` but funds have not yet been transferred to the merchant. The escrow lifecycle is represented by the invoice status state machine: `Pending → Paid → Released`. Calling `release_escrow` emits `EscrowReleasedEvent { id, merchant, amount_usdc, released_at }` and transitions the invoice to `Released` status. The actual token transfer is performed by the treasury contract (via `execute_settlement`), not the invoice contract.

### `expires_at`

A UNIX timestamp (seconds, `u64`) stored on an `Invoice` struct. Computed at creation as `ledger.timestamp() + expires_in_seconds`. During `mark_paid`, the grace window is added: if `GraceWindow > 0`, the invoice's `expires_at` is extended by that many seconds before the expiry check runs, giving the payer additional time after the nominal deadline. Invoices with `ledger.timestamp() >= expires_at` are eligible for `batch_expire`.

---

## G

### Grace window

An admin-configurable number of seconds (stored under `DataKey::GraceWindow`, instance storage, invoice contract) that is added to an invoice's `expires_at` at the moment `mark_paid` is called. Its purpose is to allow payment confirmation (`mark_paid`) to succeed on invoices that have just crossed their nominal expiry deadline — accommodating network latency or delayed off-chain payment detection without permanently extending the invoice's creation-time deadline.

Default value is `0` (no grace). Set via `set_grace_window(admin, seconds)` and read via `get_grace_window()`. If the extended deadline is still in the past when `mark_paid` runs, the call reverts with `Expired` (error code 5).

---

## H

### Hold (settlement hold)

A `SettlementStatus::OnHold` state that blocks `execute_settlement` from running. A hold is placed either explicitly via `hold_settlement(admin, settlement_id, reason)` or automatically when a dispute is raised against the settlement. Every hold carries a `SettlementHoldReason` variant (see below) that records why the hold was applied.

`release_hold(admin, settlement_id)` returns the settlement to `Pending` and clears the `hold_reason` to `SettlementHoldReason::None`.

### `SettlementHoldReason`

An enum (defined in `crates/multisig/src/lib.rs`) embedded in every `Settlement` struct:

| Variant | Meaning |
|---|---|
| `None` | Settlement is not on hold (default). |
| `ComplianceReview` | Flagged for manual AML/compliance review. |
| `FraudCheck` | Suspicious activity detected; fraud/risk system holds. |
| `KycPending` | Merchant or counterparty KYC has not been completed. |
| `AdminHold` | Manual operator hold not covered by the above categories. |

---

## I

### Idempotency (merchant nonce)

The mechanism that prevents a merchant from accidentally creating duplicate invoices. A `merchant_nonce` (`u64`) is supplied at creation time. If non-zero, the pair `(merchant_address, nonce)` is recorded under `DataKey::MerchantNonce(Address, u64)` (persistent) after a successful creation. Any subsequent creation attempt by the same merchant with the same nonce reverts with `DuplicateNonce` (error code 13). Cancelling or expiring an invoice does **not** reclaim the nonce. A value of `0` disables nonce enforcement for that invoice.

### `is_allowed` precedence

The evaluation order applied by `Compliance::is_allowed(address)`:

1. If the address is `Blocked` (and any `BlockedUntil` timestamp has not yet passed), return `false`.
2. If not `Allowed`, return `false`.
3. If `AllowedUntil` is set and `ledger.timestamp() >= AllowedUntil`, return `false` (temporary allow has expired).
4. Otherwise, return `true`.

Blocked always overrides allowed. `clear_address` must be called to remove a block before an address can be treated as allowed.

### Invoice status state machine

The ordered set of lifecycle states for an `Invoice` (defined as `InvoiceStatus` in `contracts/invoice/src/invoice.rs`):

```
Pending ──mark_paid──► Paid ──release_escrow──► Released
   │                     │
   │ cancel_invoice       └── request_refund ──► RefundRequested
   ▼                                                    │
Cancelled                                      approve_refund
                                                        ▼
                                                    Refunded

Pending ──batch_expire──► Expired  (when ledger.timestamp >= expires_at)
```

Terminal states: `Released`, `Refunded`, `Cancelled`, `Expired`. Once an invoice reaches a terminal state its status cannot be changed.

---

## M

### Merchant nonce

See [Idempotency (merchant nonce)](#idempotency-merchant-nonce).

### Merchant payout address

An optional per-merchant address override stored under `DataKey::MerchantPayoutAddress(Address)` (instance storage, treasury contract). When set, `execute_settlement` sends funds to this address instead of the `merchant_address` on the settlement record. Set by the merchant via `update_merchant_payout_address`; read via `get_merchant_payout_address`. If not set, the `merchant_address` field of the `Settlement` struct is used directly.

### Multi-sig (multisig)

The signing model used by the treasury contract. Each authorised signer carries a numeric weight (stored under `DataKey::Signer(Address)`, instance storage). An action (settlement execution, signer rotation, dispute resolution vote) proceeds only when the cumulative `approval_weight` of distinct signers who have approved meets or exceeds `Threshold`. The helper functions `require_authorized_signer`, `record_approval`, and `meets_threshold` are implemented in `crates/multisig/src/lib.rs` and shared across settlement, dispute, and rotation flows.

---

## P

### Partial settlement

A settlement variant where only a portion of the proposed `amount` is transferred in a single execution step. Proposed via `propose_partial_settlement` (an alias of `propose_settlement`), approved via `approve_partial_settlement` (which validates `partial_amount < settlement.amount`), and executed via `partially_execute_settlement`. On execution the settlement transitions to `SettlementStatus::PartiallyExecuted` rather than `Executed`. The running approved total is tracked under `DataKey::PartialApprovedTotal(u64)` (persistent).

### Pending index

A `Vec<u64>` stored under `DataKey::PendingIndex` (persistent, invoice contract) that tracks all invoice IDs currently in `Pending` status. Updated by `pending_index_add` on creation and `pending_index_remove` on any status transition out of `Pending`. Used by `batch_expire` to enumerate expirable invoices without scanning the full ID space.

---

## Q

### Quorum

Informally: the set of signer approvals whose combined weight satisfies the treasury `Threshold`. In code, quorum is reached when `meets_threshold(approval_weight, threshold)` returns `true`, i.e. `approval_weight >= threshold`. This is a simple weighted-sum model: each signer contributes their registered weight once (duplicate approvals from the same signer are ignored by `record_approval`). There is no minimum signer-count requirement beyond the weight check.

---

## S

### Settlement

The primary payment-release record in the treasury contract. Stored under `DataKey::Settlement(u64)` (persistent) as a `Settlement` struct containing: `id`, `merchant_address`, `amount`, `approvals` (list of approving signer addresses), `approval_weight` (running weight sum), `status` (`SettlementStatus`), `hold_reason` (`SettlementHoldReason`), and `proposed_at` (UNIX timestamp).

A settlement lifecycle: proposed by a signer → approved by additional signers until threshold is met → executed by any authorised signer (triggering a SEP-41 token transfer from the treasury to the merchant). A settlement may be cancelled (while `Pending`), placed on hold (while `Pending` or by dispute), or expired after `SETTLEMENT_TTL` (7 days) elapses.

### Settlement status

| Status | Meaning |
|---|---|
| `Pending` | Proposed, accumulating approvals. |
| `Executed` | Full amount transferred to merchant. |
| `PartiallyExecuted` | Partial amount transferred; original amount remains on record. |
| `PartiallySettled` | Intermediate partial-settlement state. |
| `OnHold` | Blocked from execution by a hold or open dispute. |
| `Cancelled` | Cancelled by a signer; no funds moved. |
| `Expired` | TTL elapsed; admin called `expire_settlement`. |

### Settlement TTL

The maximum lifetime of a `Pending` settlement: **7 days** (`7 * 24 * 60 * 60` seconds), defined as `SETTLEMENT_TTL` in `contracts/treasury/src/settlements.rs`. After this period elapses, an admin may call `expire_settlement` to transition the settlement to `Expired`. The check is time-based: `ledger.timestamp() > proposed_at + SETTLEMENT_TTL`.

### Signer

An `Address` registered in the treasury contract with a non-zero weight via `set_signer(admin, signer, weight)`. Signers are tracked in `DataKey::SignerList` (instance, `Vec<Address>`) and looked up by `DataKey::Signer(Address)` (instance, `u32` weight). A signer with weight `0` is effectively deactivated. Signers participate in settlement approvals, dispute resolution votes, and signer rotation proposals.

### Signer rotation

A two-step process to replace one signer with another without admin intervention. A signer calls `propose_signer_rotation(proposer, old_signer, new_signer)`, which creates a `SignerRotationProposal` under `DataKey::SignerRotation(u64)`. Other signers call `approve_signer_rotation`; when cumulative weight meets the threshold the rotation auto-executes, copying `old_signer`'s weight to `new_signer` and setting `old_signer`'s weight to `0`. A per-proposer 1-hour cooldown (enforced via `DataKey::LastRotationProposal(Address)`) prevents rotation-proposal spam.

### Signer weight

A `u32` value stored under `DataKey::Signer(Address)` (instance, treasury contract) that represents how much a given signer's approval contributes to `approval_weight`. Weights are additive; the default weighted approval model used by this protocol is a 2-of-3 configuration where each signer carries weight `1` and `Threshold` is `2`, but other configurations (e.g. unequal weights for a weighted quorum) are supported by the data model.

### Stroops

The smallest unit of a Stellar token. **1 USDC = 10 000 000 stroops** (7 decimal places), captured as `USDC_FACTOR = 10_000_000` in `contracts/invoice/src/invoice.rs`. Invoice amounts must be at least `USDC_FACTOR` stroops; amounts below this minimum are rejected with `AmountPrecision` (error code 12).

---

## T

### Threshold

The minimum `approval_weight` required to execute a settlement, auto-resolve a dispute vote, or auto-execute a signer rotation. Stored under `DataKey::Threshold` (instance, treasury contract) as a `u32`. Set during `initialize` and updatable by admin via `update_threshold`. A threshold of `0` is rejected (`ZeroThreshold`, error code 2). Execution is blocked if threshold has not been configured (`ThresholdNotConfigured`, error code 6).

### Token allowlist

An admin-managed `Vec<Address>` stored under `DataKey::TokenAllowlist` (instance, treasury contract) containing the only token contract addresses that may be used in `execute_settlement`. If the allowlist is empty, any token is accepted. If non-empty, an attempt to execute with a token not on the list reverts with `TokenNotAllowed` (error code 12). Managed via `add_allowed_token` / `remove_allowed_token`. The treasury contract itself is always excluded regardless of the allowlist (`InvalidTokenContract`, error code 11).

### Temporary allow (`AllowedUntil`)

A time-bound compliance allowance set via `Compliance::allow_address_until(admin, address, expires_at)`. Stored under `DataKey::AllowedUntil(Address)` (persistent) as a UNIX timestamp. While `ledger.timestamp() < expires_at`, `is_allowed` returns `true` for the address (assuming it is not blocked). After `expires_at` is reached, `is_allowed` returns `false` automatically — no admin action is required to expire the allowance.

---

## W

### Weighted quorum

See [Quorum](#quorum) and [Signer weight](#signer-weight). The terms are used interchangeably in this repo; "weighted quorum" emphasises that different signers may carry different weights, rather than a simple majority-of-signers model.
