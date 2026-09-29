# compliance-errors

`comebackhere-compliance-errors` (lib name `compliance_errors`) holds the
**historical, compatibility-only error ABI for the compliance contract**. It
contains exactly one type — the `#[contracterror]` enum `ComplianceError` — with
a single variant.

## Which contract uses this crate

| Consumer | Relationship |
|---|---|
| `contracts/compliance` (`comebackhere-compliance`) | Re-exports `ComplianceError` as `compliance::ComplianceError`; the contract's own entrypoints return `ContractError`, not `ComplianceError` |
| `crates/protocol-errors` (`comebackhere-protocol-errors`) | Re-exports it and wraps it in `ProtocolError::Compliance` for off-chain clients |

## Why this crate is nearly empty

`ComplianceError` has exactly one variant, `AlreadyInitialized = 1`, and has not
grown. Two reasons:

1. **It predates `ContractError`.** The compliance contract originally surfaced
   `AlreadyInitialized` through this crate. The contract has since grown its own
   `#[contracterror] ContractError` in `contracts/compliance/src/lib.rs`, and new
   variants are added there. `ComplianceError` is kept for historical
   compatibility so any client that still matches on it keeps compiling and
   keeps decoding the same `u32`.
2. **The contract leans on `require_auth()` for authorization.** An
   unauthorized admin call panics through Soroban's auth machinery rather than
   returning a domain error, so the contract needs few error codes at all.

**New compliance error variants belong in `ContractError`, not here.** Adding to
`ComplianceError` would fork the compliance code space and risk a collision with
`ContractError`'s discriminants in any client that treats both as one namespace.

## Numeric range owned

`ComplianceError` owns compliance code **`1`** (`1..=1`).

The compliance contract itself owns a second, larger range via `ContractError`
(`1..=6`), documented in [Contract codes, for reference](#contract-codes-for-reference).

> `ARCHITECTURE.md`'s "Error-Code Ranges per Contract" section covers both
> enums under a single `1..=4` heading. That heading is stale — `ContractError`
> has since grown to `1..=6` (code 6, `BulkOperationCooldown`, was appended
> under the append-only rule). The tables below are derived from the enums
> themselves, which are the ABI source of truth.

## Append-only rule

**New variants are only ever appended, at the end, with an explicit discriminant
exactly one higher than the current maximum.** Never renumber, reorder, remove,
or reuse a code. This applies to `ComplianceError` here and to `ContractError` in
the contract itself.

Two checks enforce this mechanically:

- `scripts/check-enum-ordering.sh` — walks every `#[repr(u32)]` enum in
  `contracts/` and `crates/` and fails if discriminants are not strictly
  increasing by 1 from 1.
- `scripts/check-error-code-uniqueness.sh` — fails if a discriminant is reused
  within an enum.

Both run as pre-commit hooks and in the `Pre-commit` CI workflow.

## Code table

| Code | Name | Meaning |
|---|---|---|
| 1 | `AlreadyInitialized` | `initialize` has already been called |

## Contract codes, for reference

`ContractError` lives in `contracts/compliance/src/lib.rs`, not in this crate,
but support staff looking up a code `1`–`6` reported against the compliance
contract will usually have hit this enum instead, so both tables are reproduced
here.

| Code | Name | Enum | Meaning |
|---|---|---|---|
| 1 | `Unauthorized` | `ContractError` | Caller is not the stored admin, nor the operator where the entrypoint accepts one |
| 2 | `ContractPaused` | `ContractError` | Contract is paused and the operation is blocked. Read-only entrypoints and the emergency-remediation path (`block_address*`, `clear_address`, `sweep_expired`) are **not** gated by pause |
| 3 | `AlreadyInitialized` | `ContractError` | `initialize` has already been called |
| 4 | `BatchTooLarge` | `ContractError` | Batch input exceeds `MAX_BATCH_SIZE` (50) |
| 5 | `AddressIndexFull` | `ContractError` | Tracking a new address would exceed `MAX_TRACKED_ADDRESSES` (2,000) |
| 6 | `BulkOperationCooldown` | `ContractError` | A bulk allow/block call was made before `BULK_OP_COOLDOWN_SECS` (60) elapsed since that admin's previous bulk call on the same entrypoint |

Note that codes 1–3 collide numerically with codes 1–3 in the table above. A bare
`u32` from a failed transaction is only interpretable in combination with the
contract ID it came from.

## Looking an error up in support

1. Identify the **contract ID** the failure came from — code `1` means
   `AlreadyInitialized` under `ComplianceError` and `Unauthorized` under
   `ContractError`.
2. For the compliance contract, read the meaning above and cross-reference
   `contracts/compliance/README.md` for the entrypoint and its pause policy.
3. A missing code does not rule out the compliance contract: an uninitialized
   contract, or an authorization failure, aborts with a trap rather than an
   `Err` — see the "Emergency policy" note in `contracts/compliance/src/lib.rs`.
