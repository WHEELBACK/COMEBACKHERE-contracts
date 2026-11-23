# Treasury Contract

The Treasury contract manages funds and settlements using a multi-signature approval process. It supports settlement proposals, partial settlements, disputes, and signer rotations.

---

## Flow Diagrams

The sequence diagrams below describe the primary flows supported by the treasury. Each diagram uses the actors that call the on-chain entrypoints and shows which events are emitted.

### 1. Proposal and Approval Flow

A signer proposes a settlement; one or more signers approve it; once cumulative weight meets the threshold a signer executes it.

```mermaid
sequenceDiagram
    actor Signer1
    actor Signer2
    participant Treasury

    Signer1->>Treasury: propose_settlement(signer, merchant, amount)
    Treasury-->>Signer1: settlement_id
    Note over Treasury: status = Pending<br/>approval_weight = signer1_weight
    Treasury--)Signer1: event: settlement_proposed

    Signer2->>Treasury: approve_settlement(signer2, settlement_id)
    Treasury-->>Signer2: Settlement
    Note over Treasury: approval_weight += signer2_weight
    Treasury--)Signer2: event: settlement_approved

    Signer1->>Treasury: execute_settlement(signer1, settlement_id, token)
    Note over Treasury: Checks approval_weight >= threshold
    Treasury->>Treasury: token.transfer(treasury → merchant)
    Note over Treasury: status = Executed
    Treasury--)Signer1: event: settlement_executed
```

### 2. Partial Execution Flow

A signer proposes a settlement and a partial amount is approved and transferred instead of the full amount.

```mermaid
sequenceDiagram
    actor Signer1
    actor Signer2
    participant Treasury

    Signer1->>Treasury: propose_partial_settlement(signer1, merchant, amount)
    Treasury-->>Signer1: settlement_id
    Treasury--)Signer1: event: settlement_proposed

    Signer2->>Treasury: approve_partial_settlement(signer2, settlement_id, partial_amount)
    Treasury-->>Signer2: Settlement
    Treasury--)Signer2: event: settlement_partial_approved

    Signer1->>Treasury: partially_execute_settlement(signer1, settlement_id, partial_amount, token)
    Note over Treasury: Checks approval_weight >= threshold
    Treasury->>Treasury: token.transfer(treasury → merchant, partial_amount)
    Note over Treasury: status = PartiallyExecuted
    Treasury--)Signer1: event: settlement_partial_executed
```

### 3. Hold Flow

An admin places a settlement on hold (e.g. for compliance review) and later releases it so it can proceed.

```mermaid
sequenceDiagram
    actor Admin
    actor Signer
    participant Treasury

    Signer->>Treasury: propose_settlement(signer, merchant, amount)
    Treasury-->>Signer: settlement_id
    Treasury--)Signer: event: settlement_proposed

    Admin->>Treasury: hold_settlement(admin, settlement_id, ComplianceReview)
    Note over Treasury: status = OnHold<br/>hold_reason = ComplianceReview
    Treasury--)Admin: event: settlement_held

    Note over Admin: Off-chain review completes

    Admin->>Treasury: release_hold(admin, settlement_id)
    Note over Treasury: status = Pending<br/>hold_reason = None
    Treasury--)Admin: event: settlement_released

    Signer->>Treasury: execute_settlement(signer, settlement_id, token)
    Treasury->>Treasury: token.transfer(treasury → merchant)
    Note over Treasury: status = Executed
    Treasury--)Signer: event: settlement_executed
```

### 4. Dispute Flow

A claimant raises a dispute on a pending settlement; signers vote on the resolution; once threshold is met the dispute is resolved and the settlement hold is lifted.

```mermaid
sequenceDiagram
    actor Claimant
    actor Signer1
    actor Signer2
    actor Admin
    participant Treasury

    Claimant->>Treasury: raise_dispute(claimant, settlement_id, counterparty, amount, expires_at)
    Treasury-->>Claimant: dispute_id
    Note over Treasury: settlement status = OnHold<br/>dispute status = Raised
    Treasury--)Claimant: event: dispute_raised

    Signer1->>Treasury: vote_dispute_resolution(signer1, dispute_id, in_favor_of_claimant=true)
    Treasury--)Signer1: event: dispute_resolution_voted

    Signer2->>Treasury: vote_dispute_resolution(signer2, dispute_id, in_favor_of_claimant=true)
    Note over Treasury: resolution_weight >= threshold<br/>dispute status = ResolvedClaimant<br/>settlement status = Pending
    Treasury--)Signer2: event: dispute_resolution_voted

    alt Admin resolves directly instead of signer votes
        Admin->>Treasury: resolve_dispute(admin, dispute_id, in_favor_of_claimant)
        Note over Treasury: dispute status = ResolvedClaimant or ResolvedCounterparty<br/>settlement status = Pending
        Treasury--)Admin: event: dispute_resolved
    end

    alt Admin resolves with a split
        Admin->>Treasury: resolve_dispute_split(admin, dispute_id, claimant_bps, token)
        Treasury->>Treasury: token.transfer(treasury → claimant, claimant_share)
        Treasury->>Treasury: token.transfer(treasury → counterparty, counterparty_share)
        Note over Treasury: dispute status = ResolvedSplit<br/>settlement status = Pending
        Treasury--)Admin: event: dispute_resolved_split
    end
```

### 5. Dispute Expiry Flow

If a dispute passes its `expires_at` deadline without resolution, an admin can expire it, releasing the settlement back to `Pending`.

```mermaid
sequenceDiagram
    actor Admin
    participant Treasury

    Note over Treasury: dispute status = Raised<br/>ledger.timestamp() > dispute.expires_at

    Admin->>Treasury: expire_dispute(admin, dispute_id)
    Note over Treasury: dispute status = Expired<br/>settlement status = Pending
    Treasury--)Admin: event: dispute_expired
```

---

## Entrypoints

| Function | Auth Required | Parameters | Returns | Errors |
|----------|---------------|------------|---------|--------|
| `initialize` | `admin` | `admin: Address, threshold: u32` | `Result<(), TreasuryError>` | `AlreadyInitialized`, `ZeroThreshold` |
| `set_signer` | `admin` | `admin: Address, signer: Address, weight: u32` | `()` | `Unauthorized` |
| `propose_settlement` | `signer` | `signer: Address, merchant_address: Address, amount: i128` | `u64` | `ContractPaused`, `UnauthorizedSigner`, `InvalidAmount` |
| `propose_partial_settlement` | `signer` | `signer: Address, merchant_address: Address, amount: i128` | `u64` | `ContractPaused`, `UnauthorizedSigner`, `InvalidAmount` |
| `approve_settlement` | `signer` | `signer: Address, settlement_id: u64` | `Settlement` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted` |
| `revoke_approval` | `signer` | `signer: Address, settlement_id: u64` | `Result<Settlement, TreasuryError>` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`, `ApprovalNotFound` |
| `approve_partial_settlement` | `signer` | `signer: Address, settlement_id: u64, partial_amount: i128` | `Settlement` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`, `InvalidAmount` |
| `batch_approve_settlements` | `signer` | `signer: Address, ids: Vec<u64>` | `Vec<Settlement>` | `ContractPaused`, `UnauthorizedSigner`, `BatchTooLarge`, `WeightOverflow` |
| `execute_settlement` | `signer` | `signer: Address, settlement_id: u64, token_contract: Address` | `()` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `SettlementOnHold`, `AlreadyExecuted`, `ThresholdNotConfigured`, `ThresholdNotMet`, `InvalidTokenContract`, `TokenNotAllowed` |
| `partially_execute_settlement` | `signer` | `signer: Address, settlement_id: u64, partial_amount: i128, token_contract: Address` | `()` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`, `ThresholdNotConfigured`, `ThresholdNotMet`, `InvalidTokenContract`, `InvalidAmount` |
| `cancel_settlement` | `signer` | `signer: Address, settlement_id: u64` | `()` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `SettlementNotCancellable` |
| `force_cancel_settlement` | `admin` | `admin: Address, settlement_id: u64` | `()` | `Unauthorized`, `SettlementNotFound`, `ForceCancelNotAllowed` |
| `get_pending_settlements` | None | None | `Vec<Settlement>` | None |
| `get_pending_settlements_page` | None | `start: u64, limit: u64` | `Vec<Settlement>` | None |
| `get_settlement` | None | `settlement_id: u64` | `Settlement` | `SettlementNotFound` |
| `update_threshold` (**deprecated**, #570 — bypasses the signer-change timelock; use `propose_signer_change`/`execute_signer_change` instead) | `admin` | `admin: Address, new_threshold: u32` | `Result<(), TreasuryError>` | `Unauthorized`, `ZeroThreshold` |
| `pause` | `admin` | `admin: Address` | `()` | `Unauthorized` |
| `unpause` | `admin` | `admin: Address` | `()` | `Unauthorized` |
| `raise_dispute` | `claimant` | `claimant: Address, settlement_id: u64, counterparty: Address, amount: i128` | `u64` | `ContractPaused`, `Unauthorized`, `InvalidAmount` |
| `resolve_dispute` | `admin` | `admin: Address, dispute_id: u64, in_favor_of_claimant: bool` | `()` | `Unauthorized`, `ContractPaused`, `DisputeNotFound`, `DisputeAlreadyResolved` |
| `resolve_dispute_split` | `admin` | `admin: Address, dispute_id: u64, claimant_bps: u32, token_contract: Address` | `()` | `Unauthorized`, `ContractPaused`, `DisputeNotFound`, `DisputeAlreadyResolved`, `InvalidSplitRatio` |
| `vote_dispute_resolution` | `signer` | `signer: Address, dispute_id: u64, in_favor_of_claimant: bool` | `()` | `ContractPaused`, `UnauthorizedSigner`, `DisputeNotFound`, `DisputeAlreadyResolved`, `ResolutionDirectionMismatch` |
| `deposit` | `from` | `from: Address, token_contract: Address, amount: i128` | `()` | `ContractPaused`, `Unauthorized`, `InvalidAmount` |
| `withdraw` | `to` | `to: Address, token_contract: Address, amount: i128` | `()` | `ContractPaused`, `Unauthorized`, `InvalidAmount`, `InsufficientBalance`, `DestinationNotAllowed`, `WithdrawalLimitExceeded` |
| `withdraw_all` | `admin` | `admin: Address, token_contract: Address, recipient: Address` | `()` | `Unauthorized`, `NotPaused`, `WithdrawalLimitExceeded` |
| `set_withdrawal_limit` | `admin` | `admin: Address, limit: i128, window_secs: u64` | `()` | `Unauthorized` |
| `get_withdrawal_limit` | None | None | `(i128, u64)` | None |
| `add_allowed_token` | `admin` | `admin: Address, token: Address` | `()` | `Unauthorized` |
| `remove_allowed_token` | `admin` | `admin: Address, token: Address` | `()` | `Unauthorized`, `TokenHasPendingSettlements` |
| `get_balance` | None | `address: Address, token_contract: Address` | `i128` | None |
| `get_allowed_tokens` | None | None | `Vec<Address>` | None |
| `propose_signer_rotation` | `proposer` | `proposer: Address, old_signer: Address, new_signer: Address` | `u64` | `UnauthorizedSigner` |
| `approve_signer_rotation` | `approver` | `approver: Address, rotation_id: u64` | `SignerRotationProposal` | `UnauthorizedSigner`, `RotationNotFound`, `RotationAlreadyExecuted` |
| `update_merchant_payout_address` | `merchant` | `merchant: Address, new_payout_address: Address` | `()` | `ContractPaused`, `Unauthorized` |
| `get_merchant_payout_address` | None | `merchant: Address` | `Option<Address>` | None |
| `hold_settlement` | `admin` | `admin: Address, settlement_id: u64, reason: SettlementHoldReason` | `()` | `Unauthorized`, `SettlementNotFound`, `AlreadyExecuted` |
| `release_hold` | `admin` | `admin: Address, settlement_id: u64` | `()` | `Unauthorized`, `SettlementNotFound`, `NotOnHold` |

## `batch_approve_settlements` skip semantics

`batch_approve_settlements` accepts a list of settlement IDs and approves each
`Pending` one in a single transaction. IDs that cannot be approved are **silently
skipped** rather than aborting the batch — a single bad ID never rolls back the
approvals already recorded for valid IDs.

The following ID categories are skipped without error:

| Category | Settlement status | Notes |
|----------|------------------|-------|
| Unknown ID | *(no record exists)* | The ID was never proposed or has been pruned. |
| Already executed | `Executed` or `PartiallyExecuted` | The settlement has been paid out. |
| Expired | `Expired` | The settlement TTL elapsed before it was executed. |
| Cancelled | `Cancelled` | An admin or signer cancelled the settlement. |
| On hold | `OnHold` | Blocked by an open dispute; cannot be approved until released. |

**Integrator guidance**: integrators can safely resubmit a partially-failed or
stale batch. The returned `Vec<Settlement>` contains exactly the settlements that
were approved in *this* call; previously-approved IDs in the same signer's list
are deduplicated (no double-count of weight) and produce no error.

## CLI usage examples

Replace `$TREASURY_CONTRACT`, `$ADMIN`, `$SIGNER`, `$MERCHANT`, `$TOKEN`, and `$NETWORK` with your deployed values.

### initialize

```sh
stellar contract invoke \
  --id $TREASURY_CONTRACT \
  --source $ADMIN \
  --network $NETWORK \
  -- initialize \
  --admin $ADMIN \
  --threshold 2
```

### propose_settlement

```sh
stellar contract invoke \
  --id $TREASURY_CONTRACT \
  --source $SIGNER \
  --network $NETWORK \
  -- propose_settlement \
  --signer $SIGNER \
  --merchant_address $MERCHANT \
  --amount 10000000
```

Returns the new settlement ID (`u64`).

### approve_settlement

```sh
stellar contract invoke \
  --id $TREASURY_CONTRACT \
  --source $SIGNER2 \
  --network $NETWORK \
  -- approve_settlement \
  --signer $SIGNER2 \
  --settlement_id 0
```

### execute_settlement

```sh
stellar contract invoke \
  --id $TREASURY_CONTRACT \
  --source $SIGNER \
  --network $NETWORK \
  -- execute_settlement \
  --signer $SIGNER \
  --settlement_id 0 \
  --token_contract $TOKEN
```

---

## Revoking a settlement approval (#577)

A signer who has approved a `Pending` settlement can withdraw that approval with `revoke_approval(signer, settlement_id)` any time before the settlement is executed. The signer's weight is subtracted from the settlement's `approval_weight`; if that drops the total below the threshold, `execute_settlement` fails with `ThresholdNotMet` again until enough approvals are re-collected. The call fails with `ApprovalNotFound` if `signer` has not approved, and with `AlreadyExecuted` once the settlement is no longer `Pending`. Each revocation emits `settlement_approval_revoked` so other signers and indexers are informed.

The weight subtracted is the signer's *current* weight (the counterpart of `record_approval`, which adds the weight in force at approval time), saturating at zero. If a signer's weight was changed with `set_signer` after they approved, re-check `approval_weight` on the returned `Settlement`.

## Removing an allowed token (#567)

`remove_allowed_token` refuses to remove a token that is on the allowlist while any settlement is still `Pending`, failing with `TokenHasPendingSettlements`. A settlement does not record which token it will be paid in (the token is passed to `execute_settlement`), so every `Pending` settlement is treated as potentially depending on every allowlisted token. Removing a token that is not on the allowlist is unaffected.

Recommended order of operations for operators retiring a token:

1. Stop proposing new settlements that will be paid in that token.
2. Resolve every pending settlement: execute it, `cancel_settlement` it, or let it be expired (`expire_settlement`). Use `get_pending_settlements` / `get_pending_metrics` to confirm none remain.
3. Call `remove_allowed_token`.

## Multi-token deposit accounting (#448)

Deposit balances are stored under `DataKey::Balance(holder, token_contract)` in
`crates/multisig/src/lib.rs` — every entry is keyed by *both* the depositor/withdrawer address
and the token contract, so balances for concurrently-allowlisted tokens are segregated and never
mix. `deposit`, `batch_deposit`, `withdraw`, and `get_balance` all read/write this per-(holder,
token) bucket. Note `execute_settlement`/`partially_execute_settlement` don't consult
`DataKey::Balance` at all — they pay merchants directly out of the treasury's on-chain token
balance via `token::Client::transfer`, so the deposit ledger and the settlement flow are
independent accounting paths by design.

---

## Settlement Hold Reasons

`SettlementHoldReason` (defined in `crates/multisig/src/lib.rs`) is attached to a settlement via `hold_settlement` and cleared via `release_hold`. It records why a settlement was paused so operators and auditors can see which off-chain process is responsible for lifting the hold.

| Variant | Used when | Set by (off-chain process) |
|---------|-----------|-----------------------------|
| `None` | Settlement is not on hold (default state). | N/A |
| `ComplianceReview` | A settlement is flagged for manual compliance review before funds can move. | Compliance/AML review workflow |
| `FraudCheck` | Suspicious activity is detected on the settlement or associated merchant. | Fraud detection / risk system |
| `KycPending` | The merchant or counterparty has not completed KYC verification. | KYC/identity verification process |
| `AdminHold` | An operator manually pauses a settlement for a reason not covered above. | Manual admin action |

---

## Emergency force-cancel

`force_cancel_settlement` (see #457) is an emergency-only admin override for a settlement that is permanently stuck in `Pending` or `OnHold` and unreachable through the normal `cancel_settlement`/dispute-resolution paths — for example when the signer weight required to reach quorum has become unavailable. Unlike `cancel_settlement`, it requires only `admin` auth rather than signer quorum, which is exactly why it must be used sparingly and only as a last resort: confirm normal recovery paths are genuinely unavailable before calling it. It force-cancels one specifically identified settlement by ID only — it never touches signer weights, thresholds, or other settlements. Every call emits a distinct `settlement_force_cancelled` event (separate from `settlement_cancelled`) carrying the invoking admin's address for audit purposes. If an admin-action timelock lands in this repo, this entrypoint should be gated behind it.
