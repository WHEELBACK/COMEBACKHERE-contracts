# Protocol Glossary

Definitions are grounded in this repository's actual contract semantics. Where a term maps directly to a `DataKey`, struct field, or entrypoint, the source is noted.

---

## Admin

An `Address` stored under the `Admin` `DataKey` (instance storage) in each contract. The admin is the sole authority for privileged operations such as `mark_paid`, `release_escrow`, `pause`, and `set_grace_window` (Invoice); `set_signer`, `update_threshold`, `hold_settlement` (Treasury); and `allow_address`, `block_address` (Compliance). Each contract supports a two-step admin transfer via `transfer_admin` / `accept_admin` to avoid accidental lockout.

---

## Allowlist

The set of addresses permitted to receive settlements, maintained by the Compliance contract. An address is on the allowlist when its `Allowed(Address)` persistent storage key is `true` and it is not overridden by a `Blocked` flag. The off-chain `SettlementWorkflow` calls `Compliance::is_allowed(merchant)` before calling `Treasury::execute_settlement`; a `false` result blocks the transfer. See also: **Blocked**, **Temporary Allow**.

---

## Approval Weight

The sum of `Signer(Address)` weights accumulated on a `Settlement` record via `approve_settlement` calls. Once `approval_weight >= threshold`, the settlement is eligible for execution. Each signer's weight is set by the admin via `set_signer`. See also: **Threshold**, **Quorum**.

---

## Batch Expiry

A bulk admin operation (`batch_expire`) that transitions multiple `Pending` invoices to `Expired` in a single call. Bounded to `MAX_BATCH_SIZE` (50) invoices per call to cap per-invocation storage writes. Only invoices whose `ledger.timestamp >= expires_at` are eligible.

---

## Blocked

An address marked `Blocked(Address) = true` in the Compliance contract's persistent storage. A blocked address always fails `is_allowed`, regardless of allowlist state — **Blocked overrides Allowed**. `block_address` is permitted even while the contract is paused (emergency policy). A `BlockedUntil(Address)` timestamp can make the block auto-expire.

---

## Circuit Breaker

The `Paused` instance-storage flag present on all three contracts. When `true`, most write entrypoints return `ContractPaused` immediately. The invoice and compliance contracts carve out emergency exceptions: `block_address` and `clear_address` remain callable while compliance is paused; `block_address` is callable while invoice is paused. Activated via `pause`, cleared via `unpause` (admin-only).

---

## Compliance Gate

The off-chain check — `Compliance::is_allowed(merchant)` — that must return `true` before `Treasury::execute_settlement` is called. Neither the Treasury nor the Invoice contract calls Compliance directly; the gate is enforced by the `SettlementWorkflow` intermediary. See `ARCHITECTURE.md` → Cross-Contract Call Map.

---

## Escrow

The Invoice contract's representation of funds held pending merchant release. When `mark_paid` succeeds, the invoice moves to `Paid` status, indicating payment has been received off-chain but the merchant cannot yet claim funds. The admin calls `release_escrow` to transition the invoice to `Released`, signalling that funds may be disbursed. No tokens are held inside the Invoice contract itself — "escrow" refers to the logical hold on settlement, not on-contract token custody.

---

## Expires At

The `expires_at: u64` field on an `Invoice` struct, storing a Unix timestamp (seconds since epoch) computed as `ledger.timestamp() + expires_in_seconds` at creation time. `mark_paid` rejects invoices where `ledger.timestamp() >= expires_at + grace_window`. `batch_expire` transitions invoices to `Expired` when `ledger.timestamp() >= expires_at`.

---

## Grace Window

A configurable buffer (seconds) stored under the `GraceWindow` `DataKey` (instance storage, Invoice contract). During `mark_paid`, the effective payment deadline is `expires_at + grace_window` rather than `expires_at` alone, accommodating in-flight payments that arrive just after quote expiry. Set by the admin via `set_grace_window`; defaults to `0` if never set.

---

## Hold (Settlement Hold)

A flag placed on a `Settlement` record by `Treasury::hold_settlement`, blocking `execute_settlement` with `SettlementOnHold` until `release_hold` is called. Each hold carries a `SettlementHoldReason` variant (`ComplianceReview`, `FraudCheck`, `KycPending`, `AdminHold`) so operators and auditors can identify which off-chain process must clear the hold.

---

## Idempotency / Merchant Nonce

A `merchant_nonce: u64` supplied by the merchant at invoice creation. When non-zero, the contract persists `MerchantNonce(merchant, nonce) = true` and rejects any subsequent `create_invoice` call with the same `(merchant, nonce)` pair (`DuplicateNonce`). Pass `0` to skip nonce enforcement. Prevents double-submission from storefront retries.

---

## Invoice Status State Machine

The exhaustive set of states an `Invoice` can occupy:

| Status | Description |
|---|---|
| `Pending` | Created, awaiting payment. |
| `Paid` | Payment confirmed by admin (`mark_paid`); escrow held. |
| `Released` | Escrow released by admin (`release_escrow`); terminal. |
| `Expired` | Deadline passed without payment; set by `batch_expire`. |
| `Cancelled` | Voided by merchant or admin while `Pending`. |
| `RefundRequested` | Payer opened a dispute via `request_refund`. |
| `Refunded` | Admin approved refund (`approve_refund`); terminal. |

---

## Mark Paid

The `mark_paid` entrypoint (admin-only) that transitions an invoice from `Pending` → `Paid`. It validates that the invoice is still within its effective deadline (`expires_at + grace_window`), optionally verifies `metadata_hash` and `token_address`, records `paid_at` and `payer`, and removes the invoice from the `PendingIndex`.

---

## Merchant Payout Address

An optional override stored under `MerchantPayoutAddress(Address)` in Treasury instance storage. When set, `execute_settlement` sends tokens to this address instead of the merchant's signing address. Managed via `update_merchant_payout_address`.

---

## Partial Settlement

A settlement variant (`propose_partial_settlement` / `approve_partial_settlement` / `partially_execute_settlement`) where only a portion of the proposed amount is transferred to the merchant. The partial amount must pass the same threshold and token-allowlist checks as a full settlement.

---

## Pending Index

A global `Vec<u64>` of invoice IDs stored under the `PendingIndex` `DataKey` (persistent storage, Invoice contract). Maintained by `create_invoice` (adds) and `mark_paid` / `cancel_invoice` / `batch_expire` (removes). Provides an efficient enumeration surface for off-chain expiry sweeps without scanning all invoice IDs.

---

## Quorum

Used informally in this codebase to mean the condition `approval_weight >= threshold` on a `Settlement`. There is no `Quorum` DataKey; the concept is expressed through `Threshold` (the required weight floor) and the accumulated `approval_weight` on each `Settlement` record. The treasury is configured as a 2-of-3 multi-sig in the standard deployment, meaning the threshold is set to require weight from at least 2 of the 3 authorized signers.

---

## Release Escrow

The `release_escrow` entrypoint (admin-only) that transitions a `Paid` invoice to `Released`. This is the terminal success state: it signals that the off-chain settlement workflow may proceed with fund disbursement. Emits an `escrow_released` event.

---

## Settlement

A record created in the Treasury contract by `propose_settlement`, representing a pending payout to a merchant. It accumulates signer approvals until `approval_weight >= threshold`, at which point `execute_settlement` can be called to transfer tokens from the treasury to the merchant (or their configured payout address). Stored under `Settlement(u64)` persistent storage keyed by a monotonic `SettlementCount`.

---

## Signer

An `Address` registered in Treasury instance storage under `Signer(Address)` with an associated `u32` weight. Only registered signers may call `propose_settlement`, `approve_settlement`, and `execute_settlement`. The admin manages signers via `set_signer` and the `propose_signer_rotation` / `approve_signer_rotation` rotation workflow.

---

## Temporary Allow

An allowlist entry with an expiry, stored as `AllowedUntil(Address): u64` in Compliance persistent storage. `is_allowed` returns `false` once `ledger.timestamp() >= expires_at`, even if the `Allowed` flag is still set. Created via `allow_address_until`.

---

## Threshold

The minimum cumulative approval weight required before a settlement can be executed, stored under the `Threshold` `DataKey` (instance storage, Treasury contract). Set at initialization and updatable via `update_threshold` (admin-only, cannot be set to zero). See also: **Approval Weight**, **Quorum**.

---

## Token Allowlist

A `Vec<Address>` stored under `TokenAllowlist` (instance storage, Treasury contract) listing the token contracts accepted for settlement. `execute_settlement` rejects any `token_contract` argument not present in this list (`TokenNotAllowed`). Managed by the admin via `add_allowed_token` / `remove_allowed_token`.

---

## USDC Precision / Stroops

The Invoice contract denominates amounts in stroops: `1 USDC = 10_000_000 stroops` (`USDC_FACTOR`). The `require_usdc_precision` validation guard rejects any `amount_usdc` or `gross_usdc` that is not a whole multiple of `USDC_FACTOR`, preventing sub-cent invoice amounts from entering the system (`AmountPrecision` error).
