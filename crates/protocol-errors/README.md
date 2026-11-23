# protocol-errors

`comebackhere-protocol-errors` (lib name `protocol_errors`) is the **aggregate
error surface for off-chain consumers of the whole COMEBACKHERE protocol**. It
owns no numeric codes of its own: it re-exports each contract's `#[contracterror]`
enum and wraps them in a single `ProtocolError` enum that a client can `match` on
once.

## Which contracts use this crate

**None.** This crate exists specifically to sit *outside* the on-chain dependency
graph:

| Consumer | Relationship |
|---|---|
| Off-chain integration clients / indexers | The intended consumer — one `ProtocolError` to handle errors from any contract |
| Cross-contract test fixtures (`tests/` workspace package) | Uses it to assert on errors from any contract in one match |
| `contracts/invoice`, `contracts/treasury`, `contracts/compliance`, `contracts/settlement-workflow` | **Must never depend on this crate** |

The "must never" is not stylistic. `protocol-errors` depends on the `treasury`
contract implementation crate, so a contract depending back on `protocol-errors`
would statically link treasury's wasm exports (`pause`, `unpause`, …) into its
own build and fail to link with a duplicate-symbol error. That is the exact
failure this repo already hit, which is why the rule is stated in the crate's
own rustdoc as well as here.

## What it re-exports

| Re-export | Defined in | Owned code range |
|---|---|---|
| `InvoiceError` | [`crates/invoice-errors`](../invoice-errors) — see its [code table](../invoice-errors/README.md#code-table) | `1..=23` |
| `TreasuryError` | [`crates/multisig`](../multisig) | `1..=40` |
| `ComplianceError` | [`crates/compliance-errors`](../compliance-errors) — see its [code table](../compliance-errors/README.md#code-table) | `1` |

Note the ranges are **per contract, not globally unique**. `InvoiceError` 6 and
`TreasuryError` 6 are different conditions. Only the contract ID disambiguates
them, which is exactly why the aggregate type keeps them in separate variants
rather than flattening them into one integer space.

## `ProtocolError`

```rust
pub enum ProtocolError {
    Invoice(InvoiceError),
    Treasury(TreasuryError),
    Compliance(ComplianceError),
}
```

It is a plain `#[derive(Copy, Clone, Debug, PartialEq)]` enum — **not** a
`#[contracterror]` enum, and it is never part of any contract's ABI.

| Member | Description |
|---|---|
| `From<InvoiceError>` | Converts an invoice error into `ProtocolError::Invoice` |
| `From<TreasuryError>` | Converts a treasury error into `ProtocolError::Treasury` |
| `From<ComplianceError>` | Converts a compliance error into `ProtocolError::Compliance` |
| `contract_name() -> &'static str` | Returns `"invoice"`, `"treasury"`, or `"compliance"` — the originating contract, for logging and metrics labels |

The `From` impls are what make `?` work across a call site that handles errors
from more than one contract:

```rust
use protocol_errors::ProtocolError;

fn settle(invoice: &InvoiceContractClient, id: u64) -> Result<(), ProtocolError> {
    invoice.mark_paid(&admin, &id, &payer, &None, &None)?;   // InvoiceError
    invoice.release_escrow(&admin, &id)?;                      // InvoiceError
    treasury_client.execute_settlement(&signer, &id, &token);  // no Result: traps instead
    Ok(())
}
```

`crates/compliance-client`'s `require_allowed_for_treasury` follows the same
shape, converting a failed compliance gate into
`TreasuryError::ComplianceCheckFailed` rather than an ad-hoc `panic!` or a
generic `Unauthorized` — the usual way a compliance failure actually surfaces
as a typed error in this protocol.

## Numeric range owned

**None.** `ProtocolError` has no `#[repr(u32)]` and no discriminants, so it
adds nothing to the append-only code space. The three re-exported enums keep
their own append-only rules; see [crates/invoice-errors](../invoice-errors),
[crates/compliance-errors](../compliance-errors), and
[crates/multisig](../multisig).

## Stability of `ProtocolError`'s own shape

`ProtocolError` is small and looks like an internal detail, but its three-variant
shape is deliberately treated as ABI-adjacent: `crates/protocol-errors/tests/protocol_errors_test.rs`
contains a `match` with **no wildcard arm**, so adding, removing, or renaming a
variant fails that test file to *compile* rather than passing silently. Adding a
fourth contract's error type here is therefore a deliberate, reviewed change.

## Running the tests

```sh
cargo test --package comebackhere-protocol-errors
```

Unit tests in `src/lib.rs` cover the `From` impls, the `?` operator across
boundaries, and `contract_name()`. The integration test in
`tests/protocol_errors_test.rs` covers variant exhaustiveness, contract-name
uniqueness, round-tripping, and `?` propagation.
