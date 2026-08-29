# Event Schema Reference

(#460) A single, formally-structured reference for every event emitted across all four
contracts in this workspace, for off-chain indexers to build a reliable parser against
without cross-referencing four READMEs or reading source. Every row below was derived
directly from the `env.events().publish(...)` call sites in source (not copied from
README prose), cross-checked against each contract's own README, with any discrepancy
found fixed as part of the same change (see `contracts/invoice/README.md`: `reject_refund`
and its `refund_rejected` event were implemented in source but missing from the README
entirely).

## How to read this document

Every Soroban event has two parts:

- **Topics** — a fixed-shape tuple. The first topic is always a `Symbol` naming the
  event; some events append a second topic (typically the entity ID) for cheap
  server-side filtering without decoding the data payload.
- **Data** — the event body, given as a Rust type. `Address`, `u32`, `u64`, `i128`,
  `bool`, and `Option<T>` are native Soroban SDK/XDR types. A named struct/enum links to
  its field list below the table it appears in the first time.

Topic tuples and data payloads are listed exactly as constructed in source
(`Symbol::new(&env, "...")`, the field/argument passed to `.publish(...)`), not
paraphrased.

This document should be kept in sync with source the same way `abis/*.json` is: any PR
that adds, removes, or reshapes an `env.events().publish(...)` call should update the
matching row here in the same PR. See `CONTRIBUTING.md`'s **Deprecating an entrypoint**
section for how this interacts with a planned removal.

---

## Invoice contract

Source: `contracts/invoice/src/events.rs` (the sole place invoice events are published;
all entrypoints in `contracts/invoice/src/entrypoints/` call through these helpers).

| Event | Topics | Data type | Emitted by |
|---|---|---|---|
| `invoice_created` | `(Symbol, id: u64)` | `Invoice` | `create_invoice`, `batch_create_invoice` |
| `invoice_paid` | `(Symbol, id: u64)` | `Invoice` | `mark_paid` |
| `invoice_expired` | `(Symbol, id: u64)` | `Invoice` | `batch_expire` |
| `invoice_cancelled` | `(Symbol, id: u64)` | `Invoice` | `cancel_invoice` |
| `invoice_refund_requested` | `(Symbol, id: u64)` | `Invoice` | `request_refund` |
| `refund_approved` | `(Symbol, id: u64)` | `Invoice` | `approve_refund` |
| `refund_rejected` | `(Symbol, id: u64)` | `Invoice` | `reject_refund` |
| `escrow_released` | `(Symbol, id: u64)` | `EscrowReleasedEvent` | `release_escrow` |
| `contract_paused` | `(Symbol,)` | `Address` (admin) | `pause` |
| `contract_unpaused` | `(Symbol,)` | `Address` (admin) | `unpause` |
| `invoice_amended` | `(Symbol, id: u64)` | `InvoiceAmountUpdatedEvent` | `amend_invoice` |
| `invoice_expiry_extended` | `(Symbol, id: u64)` | `InvoiceExpiryExtendedEvent` | `extend_expiry` |

**`Invoice`** (`contracts/invoice/src/invoice.rs`):
`id: u64`, `merchant: Address`, `amount_usdc: i128`, `gross_usdc: i128`,
`status: InvoiceStatus`, `expires_at: u64`, `paid_at: Option<u64>`,
`payer: MaybeAddress`, `metadata_hash: MaybeBytes`, `payment_link_hash: MaybeBytes`,
`merchant_nonce: u64`, `token_address: MaybeAddress`.

`InvoiceStatus`: `Pending | Paid | Expired | Cancelled | RefundRequested | Released | Refunded`.
`MaybeAddress`: `None | Some(Address)`. `MaybeBytes`: `None | Some(Bytes)` (`contracttype`
wrappers used in place of `Option<Address>`/`Option<Bytes>`, which soroban-sdk v20's
`contracttype` macro does not support directly).

**`EscrowReleasedEvent`**: `id: u64`, `merchant: Address`, `amount_usdc: i128`, `released_at: u64`.

**`InvoiceAmountUpdatedEvent`**: `id: u64`, `old_amount_usdc: i128`, `new_amount_usdc: i128`,
`old_gross_usdc: i128`, `new_gross_usdc: i128`.

**`InvoiceExpiryExtendedEvent`**: `id: u64`, `old_expires_at: u64`, `new_expires_at: u64`.

### Reconstructing invoice status history

Indexers should treat the invoice ID (second topic) as the stream key and order by
ledger sequence + event position. `refund_rejected` reverts status from
`RefundRequested` back to `Paid`, the only event in this contract that moves status
"backwards"; `contract_paused`/`contract_unpaused` and `invoice_amended`/
`invoice_expiry_extended` do not represent an invoice status transition and carry no
invoice ID stream key beyond what's in their own data payload (the latter two still key
by ID in the second topic; the pause events have none, being contract-wide).

---

## Compliance contract

Source: `contracts/compliance/src/lib.rs` (single-file contract; no submodules).

| Event | Topics | Data type | Emitted by |
|---|---|---|---|
| `address_allowed` | `(Symbol,)` | `Address` | `bulk_allow_addresses` (per address), `allow_address`, `allow_address_with_tier` |
| `address_blocked` | `(Symbol,)` | `Address` | `bulk_block_addresses` (per address), `block_address` |
| `address_blocked_until` | `(Symbol,)` | `(Address, u64)` — `(address, unblock_at)` | `block_address_until` |
| `address_allowed_until` | `(Symbol,)` | `(Address, u64)` — `(address, expires_at)` | `allow_address_until` |
| `address_allow_expired` | `(Symbol,)` | `Address` | `sweep_expired` (per swept address) |
| `address_cleared` | `(Symbol,)` | `Address` | `clear_address` |
| `address_revoked` | `(Symbol,)` | `Address` | `revoke_allow` |
| `admin_transfer_initiated` | `(Symbol,)` | `Address` (new_admin) | `transfer_admin` |
| `admin_transferred` | `(Symbol,)` | `Address` (new_admin) | `accept_admin` |
| `compliance_paused` | `(Symbol,)` | `Address` (admin) | `pause` |
| `compliance_unpaused` | `(Symbol,)` | `Address` (admin) | `unpause` |
| `operator_set` | `(Symbol,)` | `Address` (operator) | `set_operator` |

None of these events carry the address in a second topic — indexers must decode the
data payload (or, for the tuple-payload events, its first element) to key by address.

`AddressState` (returned by `export_snapshot`/`export_snapshot_page`, not itself
published as an event): `Allowed | Blocked | Expired`.

---

## Settlement Workflow contract

Source: `contracts/settlement-workflow/src/lib.rs`.

| Event | Topics | Data type | Emitted by |
|---|---|---|---|
| `workflow_initialized` | `(Symbol,)` | `(Address, Address)` — `(compliance_id, treasury_id)` | `initialize` |
| `settlement_workflow_executed` | `(Symbol, settlement_id: u64)` | `(Address, Address)` — `(merchant, token_contract)` | `execute_with_compliance`, `execute_with_compliance_batch` (per settlement actually executed) |

`settlement_workflow_executed` exists specifically so indexers can distinguish
compliance-gated execution from a direct `Treasury::execute_settlement` call, which
emits its own `settlement_executed` event (below) with no knowledge of the gate.

---

## Treasury contract

Source: `contracts/treasury/src/{lib,settlements,disputes,deposits,holds,signers}.rs`.

| Event | Topics | Data type | Emitted by |
|---|---|---|---|
| `treasury_initialized` | `(Symbol,)` | `Address` (admin) | `initialize` |
| `threshold_updated` | `(Symbol,)` | `u32` (new_threshold) | `update_threshold` |
| `treasury_paused` | `(Symbol,)` | `Address` (admin) | `pause` |
| `treasury_unpaused` | `(Symbol,)` | `Address` (admin) | `unpause` |
| `settlement_proposed` | `(Symbol, id: u64)` | `Settlement` | `propose_settlement`, `propose_partial_settlement` |
| `settlement_approved` | `(Symbol, settlement_id: u64)` | `Settlement` | `approve_settlement`, `batch_approve_settlements` (per settlement) |
| `settlement_partial_approved` | `(Symbol, settlement_id: u64)` | `Settlement` | `approve_partial_settlement` |
| `settlement_executed` | `(Symbol, settlement_id: u64)` | `Settlement` | `execute_settlement` |
| `settlement_partial_executed` | `(Symbol, settlement_id: u64)` | `Settlement` | `partially_execute_settlement` |
| `settlement_cancelled` | `(Symbol, settlement_id: u64)` | `Settlement` | `cancel_settlement`, `batch_cancel_settlements` (per settlement) |
| `settlement_force_cancelled` | `(Symbol, settlement_id: u64)` | `(Address, Settlement)` — `(admin, settlement)` | `force_cancel_settlement` (#457; admin-only emergency override, distinct from `settlement_cancelled`) |
| `settlement_expired` | `(Symbol, settlement_id: u64)` | `Settlement` | `expire_settlement` |
| `settlement_held` | `(Symbol, settlement_id: u64)` | `SettlementHoldReason` | `hold_settlement` |
| `settlement_released` | `(Symbol, settlement_id: u64)` | `Settlement` | `release_hold` |
| `merchant_payout_updated` | `(Symbol, merchant: Address)` | `Address` (new_payout_address) | `update_merchant_payout_address` |
| `token_allowed` | `(Symbol,)` | `Address` (token) | `add_allowed_token` |
| `token_removed` | `(Symbol,)` | `Address` (token) | `remove_allowed_token` |
| `dispute_raised` | `(Symbol, id: u64)` | `Dispute` | `raise_dispute` |
| `dispute_expired` | `(Symbol, dispute_id: u64)` | `Dispute` | `expire_dispute` |
| `dispute_resolved` | `(Symbol, dispute_id: u64)` | `Dispute` | `resolve_dispute` |
| `dispute_resolution_voted` | `(Symbol, dispute_id: u64)` | `Dispute` | `vote_dispute_resolution` |
| `deposit` | `(Symbol, from: Address)` | `i128` (amount) | `deposit`, `batch_deposit` (per token deposited) |
| `withdraw` | `(Symbol, to: Address)` | `i128` (amount) | `withdraw` |
| `treasury_drained` | `(Symbol,)` | `Address` (recipient) | `withdraw_all` |
| `signer_weight_set` | `(Symbol, signer: Address)` | `u32` (weight) | `set_signer` |
| `signer_removed` | `(Symbol,)` | `Address` (signer) | `remove_signer` |
| `rotation_proposed` | `(Symbol, id: u64)` | `SignerRotationProposal` | `propose_signer_rotation` |
| `rotation_approved` | `(Symbol, rotation_id: u64)` | `SignerRotationProposal` | `approve_signer_rotation` |
| `rotation_executed` | `(Symbol, rotation_id: u64)` | `SignerRotationProposal` | `approve_signer_rotation` (published in addition to `rotation_approved`, only when the approval reaches threshold) |
| `rotation_cancelled` | `(Symbol, rotation_id: u64)` | `SignerRotationProposal` | `cancel_rotation` |

**`Settlement`** (`crates/multisig/src/lib.rs`): `id: u64`, `merchant_address: Address`,
`amount: i128`, `approvals: Vec<Address>`, `approval_weight: u32`,
`status: SettlementStatus`, `hold_reason: SettlementHoldReason`, `proposed_at: u64`.

`SettlementStatus`: `Pending | Executed | PartiallySettled | PartiallyExecuted | OnHold | Cancelled | Expired`.
`SettlementHoldReason`: `None | ComplianceReview | FraudCheck | KycPending | AdminHold`.

**`Dispute`**: `id: u64`, `settlement_id: u64`, `claimant: Address`, `counterparty: Address`,
`amount: i128`, `status: DisputeStatus`, `resolution_approvals: Vec<Address>`,
`resolution_weight: u32`, `resolution_for_claimant: bool`, `dispute_expires_at: u64`.
`DisputeStatus`: `Raised | ResolvedClaimant | ResolvedCounterparty | Expired`.

**`SignerRotationProposal`**: `id: u64`, `old_signer: Address`, `new_signer: Address`,
`approvals: Vec<Address>`, `approval_weight: u32`, `status: RotationStatus`.
`RotationStatus`: `Pending | Executed | Cancelled`.

### Notes for indexers

- `settlement_approved`/`settlement_cancelled` are each published from two different
  entrypoints (a single-ID path and a batch path) with an identical topic/data shape —
  there is nothing in the event itself that distinguishes which path triggered it.
- `rotation_executed` and `rotation_approved` can both be published for the same
  `approve_signer_rotation` call, in that order, when the approval happens to meet
  threshold on this call.
