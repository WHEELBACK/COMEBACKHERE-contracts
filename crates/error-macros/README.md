# error-macros

`comebackhere-error-macros` (lib name `error_macros`) provides
`declare_contract_error!`, the shared macro behind every `#[contracterror]` enum in
this workspace. It exists to stop four crates from hand-repeating the same
three-attribute block — and to stop those blocks from drifting apart, which they
had already done (the derive lists were inconsistent before this crate existed).

Full API documentation, expansion semantics, and the invariants a change to the
macro must preserve live in the crate-level rustdoc (`crates/error-macros/src/lib.rs`,
viewable with `cargo doc -p comebackhere-error-macros --open`).

## Quick reference

```rust
use error_macros::declare_contract_error;

declare_contract_error! {
    /// Error codes for the widget contract. Append-only; discriminants are
    /// on-chain ABI. See `scripts/check-enum-ordering.sh`.
    pub enum WidgetError {
        Unauthorized = 1,
        NotFound = 2,
    }
}
```

expands to:

```rust
/// Error codes for the widget contract. Append-only; discriminants are
/// on-chain ABI. See `scripts/check-enum-ordering.sh`.
#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum WidgetError {
    Unauthorized = 1,
    NotFound = 2,
}
```

Note there is no `use soroban_sdk::contracterror;` — the macro emits a
fully-qualified attribute, so the calling crate needs no extra import.

## Which enums use it

| Enum | Crate | Range |
|---|---|---|
| `InvoiceError` | `crates/invoice-errors` | `1..=23` |
| `TreasuryError` | `crates/multisig` | `1..=40` |
| `ComplianceError` | `crates/compliance-errors` | `1` |
| `ContractError` | `contracts/compliance` | `1..=6` |

A hand-written `#[contracterror]` enum is still valid and still checked; the
macro is a convention, not a requirement.

## Running the tests

```sh
cargo test --package comebackhere-error-macros
```

`tests/macro_expansion.rs` asserts the macro is *transparent*: explicit
discriminants survive verbatim, the generated enum converts to and from
`soroban_sdk::Error` with the right wire code, the `contractspecv0` entry it
publishes decodes back to the same name/code pairs, and every derive the macro
documents is present (enforced through generic bounds, so a missing derive is a
compile error rather than a silently narrower type).

## The CI guards behind it

| Script | What it protects | Macro-aware? |
|---|---|---|
| `scripts/check-enum-ordering.sh` | Discriminants stay append-only, no gaps or reordering | Yes — `declare_contract_error!` is treated as an equivalent `#[repr(u32)]` block opener |
| `scripts/check-error-code-uniqueness.sh` | No discriminant is reused within an enum | Yes — it matches on `pub enum …Error {`, which the macro preserves |
| `scripts/check-enum-doc-comments.sh` | Every error enum carries an enum-level `///` | Yes — the invocation opens the candidate block and the doc is expected above the `pub enum` line |

Both scripts are pre-commit hooks and run in the `Pre-commit` CI workflow. If you
add a new error enum behind this macro, all three apply to it automatically.
