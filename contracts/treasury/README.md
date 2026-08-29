# Treasury Contract

The Treasury contract manages funds and settlements using a multi-signature approval process. It supports settlement proposals, partial settlements, disputes, and signer rotations.

## Entrypoints

| Function | Auth Required | Parameters | Returns | Errors |
|----------|---------------|------------|---------|--------|
| `initialize` | `admin` | `admin: Address, threshold: u32` | `Result<(), TreasuryError>` | `AlreadyInitialized`, `ZeroThreshold` |
| `set_signer` | `admin` | `admin: Address, signer: Address, weight: u32` | `()` | `Unauthorized` |
| `propose_settlement` | `signer` | `signer: Address, merchant_address: Address, amount: i128` | `u64` | `ContractPaused`, `UnauthorizedSigner`, `InvalidAmount` |
| `propose_partial_settlement` | `signer` | `signer: Address, merchant_address: Address, amount: i128` | `u64` | `ContractPaused`, `UnauthorizedSigner`, `InvalidAmount` |
| `approve_settlement` | `signer` | `signer: Address, settlement_id: u64` | `Settlement` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted` |
| `approve_partial_settlement` | `signer` | `signer: Address, settlement_id: u64, partial_amount: i128` | `Settlement` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`, `InvalidAmount` |
| `execute_settlement` | `signer` | `signer: Address, settlement_id: u64, token_contract: Address` | `()` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `SettlementOnHold`, `AlreadyExecuted`, `ThresholdNotConfigured`, `ThresholdNotMet`, `InvalidTokenContract`, `TokenNotAllowed` |
| `partially_execute_settlement` | `signer` | `signer: Address, settlement_id: u64, partial_amount: i128, token_contract: Address` | `()` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`, `ThresholdNotConfigured`, `ThresholdNotMet`, `InvalidTokenContract`, `InvalidAmount` |
| `cancel_settlement` | `signer` | `signer: Address, settlement_id: u64` | `()` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `SettlementNotCancellable` |
| `force_cancel_settlement` | `admin` | `admin: Address, settlement_id: u64` | `()` | `Unauthorized`, `SettlementNotFound`, `ForceCancelNotAllowed` |
| `get_pending_settlements` | None | None | `Vec<Settlement>` | None |
| `get_pending_settlements_page` | None | `start: u64, limit: u64` | `Vec<Settlement>` | None |
| `get_settlement` | None | `settlement_id: u64` | `Settlement` | `SettlementNotFound` |
| `update_threshold` | `admin` | `admin: Address, new_threshold: u32` | `Result<(), TreasuryError>` | `Unauthorized`, `ZeroThreshold` |
| `pause` | `admin` | `admin: Address` | `()` | `Unauthorized` |
| `unpause` | `admin` | `admin: Address` | `()` | `Unauthorized` |
| `raise_dispute` | `claimant` | `claimant: Address, settlement_id: u64, counterparty: Address, amount: i128` | `u64` | `ContractPaused`, `Unauthorized`, `InvalidAmount` |
| `resolve_dispute` | `admin` | `admin: Address, dispute_id: u64, in_favor_of_claimant: bool` | `()` | `Unauthorized`, `ContractPaused`, `DisputeNotFound`, `DisputeAlreadyResolved` |
| `vote_dispute_resolution` | `signer` | `signer: Address, dispute_id: u64, in_favor_of_claimant: bool` | `()` | `ContractPaused`, `UnauthorizedSigner`, `DisputeNotFound`, `DisputeAlreadyResolved`, `ResolutionDirectionMismatch` |
| `deposit` | `from` | `from: Address, token_contract: Address, amount: i128` | `()` | `ContractPaused`, `Unauthorized`, `InvalidAmount` |
| `withdraw` | `to` | `to: Address, token_contract: Address, amount: i128` | `()` | `ContractPaused`, `Unauthorized`, `InvalidAmount`, `InsufficientBalance` |
| `add_allowed_token` | `admin` | `admin: Address, token: Address` | `()` | `Unauthorized` |
| `remove_allowed_token` | `admin` | `admin: Address, token: Address` | `()` | `Unauthorized` |
| `get_balance` | None | `address: Address` | `i128` | None |
| `get_allowed_tokens` | None | None | `Vec<Address>` | None |
| `propose_signer_rotation` | `proposer` | `proposer: Address, old_signer: Address, new_signer: Address` | `u64` | `UnauthorizedSigner` |
| `approve_signer_rotation` | `approver` | `approver: Address, rotation_id: u64` | `SignerRotationProposal` | `UnauthorizedSigner`, `RotationNotFound`, `RotationAlreadyExecuted` |
| `update_merchant_payout_address` | `merchant` | `merchant: Address, new_payout_address: Address` | `()` | `ContractPaused`, `Unauthorized` |
| `get_merchant_payout_address` | None | `merchant: Address` | `Option<Address>` | None |
| `hold_settlement` | `admin` | `admin: Address, settlement_id: u64, reason: SettlementHoldReason` | `()` | `Unauthorized`, `SettlementNotFound`, `AlreadyExecuted` |
| `release_hold` | `admin` | `admin: Address, settlement_id: u64` | `()` | `Unauthorized`, `SettlementNotFound`, `NotOnHold` |

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
