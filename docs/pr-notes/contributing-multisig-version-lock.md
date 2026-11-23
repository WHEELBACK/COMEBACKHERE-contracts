# Implementation notes: document the multisig ABI version-lock process

## Issue

`contracts/treasury/tests/multisig_version_lock_test.rs` describes, in its own
doc comment, a deliberate review process that must be followed any time
`crates/multisig`'s ABI-relevant types change: update every exhaustive
match/destructure in that test file first, confirm the file compiles again,
and only then bump `crates/multisig/Cargo.toml`'s version and the test's
`EXPECTED_MULTISIG_VERSION` constant together.

That process previously existed only as a comment inside the test file. A
contributor about to touch `crates/multisig` for an unrelated reason had no
way to discover the process exists unless they already knew to look inside
that specific test — rather than in `CONTRIBUTING.md`, the natural place to
look for repo-wide conventions before touching a crate shared across
multiple contracts.

## What changed

- Added a new `## Changing crates/multisig` section to `CONTRIBUTING.md`,
  placed directly after the existing `## ABI snapshots` section (the other
  place in the document that already discusses ABI stability).
- The new section summarizes the four-step review process (change the crate,
  attempt to compile treasury's tests, fix every exhaustive match until it
  compiles, then bump the crate version and `EXPECTED_MULTISIG_VERSION`
  together) and links directly to
  `contracts/treasury/tests/multisig_version_lock_test.rs` as the
  authoritative enforcement mechanism, rather than duplicating its full
  detail.

## Why this placement

The test file remains the single source of truth for *which* types are
locked and *how* the compile-time guard works — CONTRIBUTING.md only needs
to make the existence of that process discoverable and point back at it, per
the issue's explicit requirement not to duplicate the test file's detail.

## Verification

This is a documentation-only change (no source files were touched), so it
does not affect `cargo test --all`, `cargo clippy -- -D warnings`, or
`cargo fmt --all -- --check`. The new section was checked by hand against
`multisig_version_lock_test.rs`'s current doc comment and test bodies to
confirm it accurately reflects the real process (see the four numbered steps
in `multisig_version_lock_test.rs`'s header comment and the assertion
message in `multisig_crate_version_is_pinned`).
