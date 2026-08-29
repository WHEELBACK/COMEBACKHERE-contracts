# Implementation notes: exhaustive-match ABI guard for ProtocolError

## Issue

`multisig_version_lock_test.rs` exhaustively pins `TreasuryError` and several
multisig `#[contracttype]` enums via non-exhaustive-match-fails-to-compile
guards, but `ProtocolError` in `crates/protocol-errors` — which wraps
`InvoiceError`, `TreasuryError`, and `ComplianceError` into the single type
off-chain integration clients are meant to use — had no equivalent guard
anywhere in the workspace. A silent change to `ProtocolError`'s shape (a
variant removed, renamed, or reordered) would break its intended audience
with nothing catching the drift at compile time.

## What changed

- Added `assert_protocol_error_exhaustive` and
  `protocol_error_variants_are_exhaustive` to
  `crates/protocol-errors/tests/protocol_errors_test.rs`: a `match` over
  `ProtocolError`'s three variants (`Invoice`, `Treasury`, `Compliance`)
  with no wildcard arm. Adding a fourth contract's error type to
  `ProtocolError` will fail this file to compile until the new arm is added
  here.

## Placement decision

The issue asked us to weigh extending `contracts/treasury`'s existing
`multisig_version_lock_test.rs` against adding a guard directly in
`crates/protocol-errors`. We chose the latter:

- `ProtocolError` is defined in `crates/protocol-errors`, not in treasury,
  and wraps errors from three separate contracts (`invoice`, `treasury`,
  `compliance`) — it isn't treasury-specific, so a treasury test file is the
  wrong crate boundary for it.
- `crates/protocol-errors/tests/protocol_errors_test.rs` already exists,
  already imports all three wrapped error types, and already exercises
  `ProtocolError`'s `From` conversions and `contract_name()` — adding the
  guard here keeps ABI-relevant tests for a type colocated with the crate
  that owns it, rather than spreading protocol-errors' own ABI guarantees
  into an unrelated contract's test suite.
- This mirrors the general principle `multisig_version_lock_test.rs` itself
  established (pin ABI-relevant shapes with a compile-time exhaustive
  match), applied at the crate that actually defines the type being pinned.

## Verification

Not run as part of this change; verify before merge with
`cargo test -p comebackhere-protocol-errors` to confirm the new test
compiles and passes against the current three-variant `ProtocolError` shape.
