# Changelog — `crates/multisig`

All notable changes to this crate are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> **ABI stability notice:** `TreasuryError` discriminants are part of the
> on-chain ABI. Clients match on numeric codes returned by deployed contracts,
> so discriminants must **only be appended** — never renumbered or removed.
> This invariant is enforced automatically by `scripts/check-enum-ordering.sh`
> on every CI run (see issue #74).

---

## [0.4.0] — Timelocked signer/threshold changes (issue #447)

### Added

- Three new `TreasuryError` variants appended at discriminants 38–40:
  - `SignerChangeTooEarly = 38` — returned when `execute_signer_change` is called before the 24 h delay has elapsed.
  - `SignerChangeNotFound = 39` — returned when a `change_id` does not exist.
  - `SignerChangeAlreadyFinalised = 40` — returned when a proposal has already been executed or cancelled.
- Duplicate discriminant entries (34/35/36 appeared twice in the previous source as a merge artifact) are removed; `WorkflowNotRegisteredSigner` is now the sole holder of 34.
- New `contracttype` enums: `SignerChangeKind`, `SignerChangeStatus`.
- New `contracttype` struct: `SignerChangeProposal`.
- New `DataKey` variants: `SignerChangeCount`, `SignerChange(u64)`.

---

## [0.2.0] — Treasury WASM-size fix

### Context

The compiled `contracts/treasury` WASM was approaching the **65 536-byte (64 KiB)**
hard ceiling imposed by Soroban at the protocol level (see the Contract Size Budget
section of `ARCHITECTURE.md`). Profiling showed that raw `panic!` call sites with
inline string literals were a material contributor: each unique string is baked into
the binary's data segment.

The fix converted every raw `panic!` in `contracts/treasury` to a typed
`panic_with_error!(env, TreasuryError::Variant)` call. This replaces the string
payload with a compact numeric error code, shrinking the data segment and bringing
the WASM well below the size ceiling.

Because the conversion introduced **14 new `TreasuryError` variants** (discriminants
20–33), the crate version was bumped from `0.1.0` to `0.2.0`. The bump was
enforced mechanically: `contracts/treasury/tests/multisig_version_lock_test.rs`
pins `EXPECTED_MULTISIG_VERSION` and exhaustively matches every
`TreasuryError` variant at compile time, so any unreviewed addition would have
caused a compile failure until `EXPECTED_MULTISIG_VERSION` was updated to `"0.2.0"`.

### Added

14 new `TreasuryError` variants appended at the end of the enum (discriminants 20–33).
Variants 1–19 and all struct/enum shapes are **unchanged**.

| Discriminant | Variant name | Raw `panic!` message it replaces |
|---|---|---|
| 20 | `ArithmeticOverflow` | `"arithmetic overflow"` |
| 21 | `DisputeNotFound` | `"dispute not found"` |
| 22 | `DisputeAlreadyResolved` | `"dispute already resolved"` |
| 23 | `ResolutionDirectionMismatch` | `"resolution direction mismatch"` |
| 24 | `BatchTooLarge` | `"batch too large"` |
| 25 | `WeightOverflow` | `"weight overflow"` |
| 26 | `SettlementNotCancellable` | `"settlement not cancellable"` |
| 27 | `TtlNotElapsed` | `"ttl not elapsed"` |
| 28 | `AllowlistFull` | `"allowlist full"` |
| 29 | `NotOnHold` | `"settlement not on hold"` |
| 30 | `DestinationNotAllowed` | `"destination not allowed"` |
| 31 | `InsufficientBalance` | `"insufficient balance"` |
| 32 | `NotPaused` | `"contract not paused"` |
| 33 | `RotationProposalCooldown` | `"rotation proposal cooldown active"` |

### Unchanged

- `TreasuryError` variants 1–19 retain their discriminant values and names.
- All shared struct definitions (`Settlement`, `Dispute`, `SignerRotationProposal`)
  are field-for-field identical to `0.1.0`.
- All shared enum definitions (`SettlementHoldReason`, `SettlementStatus`,
  `DisputeStatus`, `RotationStatus`) retain all variants in the same order.
- All four helper functions (`signer_weight`, `require_authorized_signer`,
  `record_approval`, `meets_threshold`) retain identical signatures and semantics.
- `DataKey` variants are unchanged.

---

## [0.1.0] — Initial release

### Added

#### `TreasuryError` (discriminants 1–19)

| Discriminant | Variant name | Meaning |
|---|---|---|
| 1 | `AlreadyInitialized` | `initialize` has already been called |
| 2 | `ZeroThreshold` | Approval threshold cannot be zero |
| 3 | `SettlementNotFound` | No settlement exists for the given ID |
| 4 | `AlreadyExecuted` | Settlement is already executed, cancelled, or expired |
| 5 | `ThresholdNotMet` | Approval weight is below the required threshold |
| 6 | `ThresholdNotConfigured` | Threshold has not been set |
| 7 | `InvalidAmount` | Amount is zero or negative |
| 8 | `ContractPaused` | Contract is paused and the operation is blocked |
| 9 | `Unauthorized` | Caller is not the admin |
| 10 | `UnauthorizedSigner` | Caller is not an authorised signer (zero weight) |
| 11 | `InvalidTokenContract` | Token contract is the treasury itself |
| 12 | `TokenNotAllowed` | Token is not on the settlement allowlist |
| 13 | `RotationNotFound` | No signer rotation proposal for the given ID |
| 14 | `RotationAlreadyExecuted` | Rotation proposal already executed or cancelled |
| 15 | `SettlementOnHold` | Settlement is on hold and cannot be executed |
| 16 | `DisputeNotExpired` | Dispute expiry timestamp has not been reached |
| 17 | `AlreadyOnHold` | Settlement is already on hold |
| 18 | `ThresholdUnreachable` | Registered signer weights cannot sum to meet threshold |
| 19 | `ComplianceCheckFailed` | Compliance allow-list check failed for the destination |

#### Shared types

| Type | Kind | Description |
|---|---|---|
| `Settlement` | struct | Settlement record: ID, merchant, amount, approvals, weight, status, hold reason, timestamp |
| `Dispute` | struct | Dispute record: ID, settlement ID, claimant, counterparty, amount, status, resolution approvals/weight, expiry |
| `SignerRotationProposal` | struct | Rotation proposal: ID, old/new signer, approvals, weight, status |
| `SettlementHoldReason` | enum | `None`, `ComplianceReview`, `FraudCheck`, `KycPending`, `AdminHold` |
| `SettlementStatus` | enum | `Pending`, `Executed`, `PartiallySettled`, `PartiallyExecuted`, `OnHold`, `Cancelled`, `Expired` |
| `DisputeStatus` | enum | `Raised`, `ResolvedClaimant`, `ResolvedCounterparty`, `Expired` |
| `RotationStatus` | enum | `Pending`, `Executed`, `Cancelled` |
| `DataKey` | enum | Storage keys: `Admin`, `Threshold`, `SettlementCount`, `Settlement(u64)`, `Signer(Address)`, `Paused`, `DisputeCount`, `Dispute(u64)`, `Balance(Address)`, `TokenAllowlist`, `RotationCount`, `SignerRotation(u64)`, `MerchantPayoutAddress(Address)`, `SignerList`, `WithdrawalAllowlist`, `LastRotationProposal(Address)`, `PartialApprovedTotal(u64)` |

#### Helper functions

| Function | Signature | Description |
|---|---|---|
| `signer_weight` | `(env: &Env, signer: &Address) -> u32` | Returns the approval weight for `signer`, or `0` if not registered. Never panics. |
| `require_authorized_signer` | `(env: &Env, signer: &Address)` | Requires `signer` to authenticate and have non-zero weight. Panics with `UnauthorizedSigner` if not. |
| `record_approval` | `(env: &Env, approvals: &mut Vec<Address>, weight: &mut u32, signer: &Address)` | Deduplicates and accumulates signer weight into `weight`. Panics with `WeightOverflow` on `u32` overflow. |
| `meets_threshold` | `(weight: u32, threshold: u32) -> bool` | Returns `true` if `weight >= threshold`. |
