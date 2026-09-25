# invoice-errors

`comebackhere-invoice-errors` (lib name `invoice_errors`) is the **stable error ABI
for the invoice contract**. It contains exactly one type — the
`#[contracterror]` enum `InvoiceError` — whose numeric discriminants are part of
the COMEBACKHERE on-chain interface.

## Which contract uses this crate

| Consumer | Relationship |
|---|---|
| `contracts/invoice` (`comebackhere-invoice`) | Defines every entrypoint's return type in terms of `InvoiceError`, and raises every code below |
| `crates/protocol-errors` (`comebackhere-protocol-errors`) | Re-exports it and wraps it in `ProtocolError::Invoice` for off-chain clients |
| `contracts/treasury`, `contracts/compliance`, `contracts/settlement-workflow` | Do **not** depend on this crate |

Splitting the error enum into its own crate is what lets `protocol-errors`
aggregate the protocol's error surface without pulling in a contract
implementation (and therefore without dragging foreign wasm exports into a
contract build). It also gives the error codes a single source of truth that is
independent of contract logic churn.

## Numeric range owned

`InvoiceError` owns the invoice code range **`1..=23`** (`#[repr(u32)]`).

> `ARCHITECTURE.md`'s "Error-Code Ranges per Contract" section still lists this
> range as `1..=21`. Codes 22 and 23 were appended afterwards, following the
> append-only rule below, and the table here is derived from the enum itself —
> which is the ABI source of truth. Treat the enum, not the `ARCHITECTURE.md`
> heading, as authoritative.

## Append-only rule

**New variants are only ever appended, at the end, with an explicit discriminant
exactly one higher than the current maximum.** Never renumber, reorder, remove,
or reuse a code.

The numeric value is what on-chain callers and off-chain indexers match on;
Soroban surfaces a `contracterror` as a `u32` over the wire, so a client that
compiled against `NotFound = 6` would silently mis-handle `NotFound` if it were
renumbered. Two checks enforce this mechanically:

- `scripts/check-enum-ordering.sh` — walks every `#[repr(u32)]` enum in
  `contracts/` and `crates/` and fails if the discriminants are not strictly
  increasing by 1 from 1.
- `scripts/check-error-code-uniqueness.sh` — fails if a discriminant is ever
  reused within an enum.

Both run as pre-commit hooks and in the `Pre-commit` CI workflow. To add a code:

```rust
// crates/invoice-errors/src/lib.rs
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum InvoiceError {
    // ... existing variants, unchanged ...
    HashTooLong = 23,
    SomeBrandNewCondition = 24, // <- append here, never in the middle
}
```

## Code table

| Code | Name | Meaning |
|---|---|---|
| 1 | `Unauthorized` | Caller is not the expected admin or merchant |
| 2 | `ContractPaused` | Contract is paused and the operation is blocked |
| 3 | `InvalidAmount` | Amount is zero, negative, or `gross_usdc < amount_usdc` |
| 4 | `NotPending` | Invoice status is not `Pending` for the required transition |
| 5 | `Expired` | Payment window (including grace period) has elapsed |
| 6 | `NotFound` | No invoice exists for the given ID |
| 7 | `AlreadyInitialized` | `initialize` has already been called |
| 8 | `ZeroDuration` | `expires_in_seconds` is zero |
| 9 | `ExpiryOverflow` | `expires_at` arithmetic overflowed `u64` |
| 10 | `NotPaid` | Invoice is not in `Paid` status |
| 11 | `NotReleased` | Invoice has not been released from escrow |
| 12 | `AmountPrecision` | Amount below the minimum USDC unit (`< 10_000_000` stroops) |
| 13 | `DuplicateNonce` | Merchant nonce has already been used |
| 14 | `ExpiryTooLong` | `expires_in_seconds` exceeds the 5-year maximum (`MAX_EXPIRY_SECONDS`) |
| 15 | `MetadataMismatch` | Supplied `metadata_hash` does not match the stored hash |
| 16 | `NoPendingAdmin` | No pending admin transfer to accept |
| 17 | `InvalidPaymentLinkHash` | `payment_link_hash` is present but not exactly 32 bytes |
| 18 | `NotRefundRequested` | Invoice is not in `RefundRequested` status |
| 19 | `TokenMismatch` | Supplied payment token does not match the invoice's expected token |
| 20 | `BatchTooLarge` | Batch input exceeds `MAX_BATCH_SIZE` (50) |
| 21 | `CooldownActive` | `create_invoice` called again before the per-merchant creation cooldown elapsed |
| 22 | `InvoiceCountOverflow` | Running invoice counter could not be advanced (nonce exhausted) |
| 23 | `HashTooLong` | Optional hash field exceeds `MAX_HASH_BYTES` (64) |

## Looking an error up in support

Support staff receiving a user-reported error get a bare `u32` from the failed
transaction. Find the row with that code, then:

1. Check the code is in `1..=23`. Anything else is not an `InvoiceError` — see
   `crates/multisig` for `TreasuryError` and `contracts/compliance` for
   `ContractError`.
2. Read the "Meaning" column above for the condition that triggers it.
3. Cross-reference `contracts/invoice/README.md` for the entrypoint that can
   raise it.

Note that a few invoice paths also abort with a trap instead of returning an
`Err` — an uninitialized contract (`unwrap()` on the missing `DataKey::Admin`)
and the arithmetic-overflow guards, which panic with `"ArithmeticOverflow"`.
Those arrive in support reports as a failed transaction with **no** error code,
so a missing code does not rule out the invoice contract.
