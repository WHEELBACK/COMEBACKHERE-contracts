## Summary

Four independent test/hardening items across the settlement-workflow, compliance
and multisig crates, all landing on one branch so a single PR can close them
together. Two are pure test additions, one adds a two-step admin transfer to the
settlement-workflow contract, and two smaller fixes were needed to make the test
work actually run.

The most consequential finding: **the three existing `multisig` unit tests were
never executing** — see #623 below.

## Changes

### #621 — Two-step admin transfer on the settlement workflow

The workflow's admin is fixed at initialization, so a lost or leaked key meant
redeploying. This mirrors the pattern invoice and compliance already use.

- `initialize` now takes an `admin` argument (which must authorize the call).
- `transfer_admin` nominates a successor and emits `admin_transfer_initiated`.
  **The role does not move yet.**
- `accept_admin` completes the handover, requires the nominee's own auth, clears
  the nomination, and emits `admin_transferred`. With no nomination outstanding
  it returns the new typed `TreasuryError::NoPendingAdmin` rather than silently
  succeeding and promoting an arbitrary caller.
- `NoPendingAdmin = 41` is **appended** to the shared `TreasuryError` rather than
  given a workflow-local error enum, following the existing precedent of
  `ComplianceCheckFailed = 19` — the workflow already uses `TreasuryError` as its
  single error type. Appended, not renumbered: existing on-chain discriminants are
  unchanged, and `scripts/check-enum-ordering.sh` passes.
- Callers updated for the new `initialize` signature: `scripts/init-contracts.sh`,
  `.github/workflows/testnet-deploy.yml`, and the existing tests. Both deploy
  paths pass the address they already use for the other contracts' admins.
- `abis/settlement-workflow.json` regenerated via `scripts/regen-abis.sh`.
- `multisig_version_lock_test.rs` updated — its exhaustive `match` on
  `TreasuryError` failed to compile on the new variant, which is exactly the
  guard working as designed. The treasury ABI snapshot is unaffected (no treasury
  function or event changed).

New `admin_rotation_test.rs` (14 tests) covers the failure modes a single-step
implementation would have: the role moving on `transfer_admin` alone, acceptance
by an address that is not the nominee, `accept_admin` with nothing outstanding, a
completed handover being replayable, the outgoing admin retaining power after
handover, and both steps emitting **exactly one** event each. `transfer_admin` is
asserted to emit *only* the initiation event, so an indexer treating
`admin_transferred` as "admin is now X" cannot act on a handover the nominee never
accepted.

### #620 — Re-initialisation guard on the workflow

The guard already existed in `src/lib.rs` (`AlreadyInitialized` on a repeat call);
the gap was the test. The existing coverage in `settlement_workflow_test.rs`
re-initialises with the **same** addresses, so it would still pass against an
implementation that blindly overwrote both keys on every call. The dangerous case
is re-initialising with *different* addresses.

New `workflow_reinit_guard_test.rs` (8 tests) drives that case: the call must fail
with a typed error, the stored links must be unchanged, a merchant approved by the
*decoy* compliance instance must still be rejected, and the workflow must still
pay out from the *original* pair afterwards. The #620 tests are additionally
extended now that an admin exists — a rejected re-init must leave the stored
admin, the pinned instances **and** the pending-admin slot untouched, and
authority must still sit with the original admin afterwards.

### #613 — Fuzz target for `is_allowed`

`is_allowed` is a pure read over four storage keys, but the values in those keys
are only reachable through a chain of admin mutations and the passage of ledger
time. A fuzzer picking a single final state would never exercise the transitions,
and several interesting bugs live *in* the transitions: `block_address`
deliberately does not clear a previously written `BlockedUntil`, `clear_address`
deliberately does not clear `AllowedUntil`, and `require_not_paused` gates only
some of the allow/revoke mutations.

The harness therefore fuzzes an ordered **sequence** of mutations and time jumps
and re-checks `is_allowed` against a model after every step. The model mirrors the
four storage keys plus the `Paused` flag, and `reference_is_allowed` re-derives the
precedence rules from the README with no shared code with the contract — the
differential style already established in `is_allowed_differential_test.rs`,
extended there from a single final state to a whole sequence.

The highest-value generator is `JumpToBoundary`, which lands the clock exactly on,
just before, or just after an expiry a previous op wrote. That is where
`now >= unblock_at` (inclusive) and `now < expires_at` (exclusive) diverge.

Failures dump the full op log with per-step subject/time/pause state, so a
diverging sequence is directly liftable into a regression test.

### #623 — Unit tests for the `multisig` crate

31 unit tests across `signer_weight`, `require_authorized_signer`,
`record_approval` and `meets_threshold`, covering the edge cases the issue calls
out: zero weights, duplicate approvals (adjacent, non-adjacent, and against a
pre-populated vector), threshold equal to total registered weight, signers removed
via `remove_signer` vs. zeroed, per-address weight independence, `u32::MAX`
boundaries, a typed `WeightOverflow` rather than a bare panic, and
`require_authorized_signer` requiring the *signer's own* auth.

### Two incidental fixes

- **The three existing `multisig` tests never ran.** They were gated on
  `#[cfg(feature = "testutils")]`, but nothing in the workspace enables
  `multisig/testutils`, and a crate's own feature cannot be switched on for its own
  unit tests without a non-default `cargo test --features ...`. `cargo test -p
  multisig` reported `running 0 tests`. The gate is now `#[cfg(test)]` alone, with
  `soroban-sdk/testutils` coming from a dev-dependency so wasm32 release builds are
  unaffected (soroban-sdk hard-disables that feature for wasm32, and `multisig` is
  a path dependency of three contracts). Had they run, all three would also have
  failed — they use instance storage outside a contract frame.
- `contracts/compliance/fuzz/` now ships a `Cargo.lock` copied from the root
  workspace. A standalone workspace resolves independently, and the unpinned
  resolve selects `ed25519-dalek` 3.x, which does not satisfy
  `soroban-env-host`'s `ChaCha20Rng: CryptoRng` bound. The `invoice` and
  `settlement-workflow` fuzz crates predate this and have no lockfile, so **both
  fail to build today** — worth fixing the same way, not done here.
- `.gitignore` excludes `contracts/*/fuzz/corpus/`, since a fuzz run writes ~540
  generated inputs into the working tree.

## Tested

`cargo test --workspace` — full suite green, no snapshot drift.

- `cargo test -p comebackhere-multisig` — 31 unit tests + 4 doctests, up from
  `running 0 tests`.
- `cargo test -p comebackhere-settlement-workflow` — 14 admin-rotation tests, 8
  re-init guard tests, 7 existing, all passing.
- Fuzz run, 5 minutes: **40,314 executions at 133 exec/s**, corpus grew to 538
  inputs, no divergences and no crashes — so there is no failing sequence to
  promote to a regression test yet. The harness panics on a model mismatch rather
  than only tolerating `Err`, so a future regression surfaces as a failing case
  rather than as silence.
- `cargo fmt --all -- --check` and `cargo clippy --workspace -- -D warnings` —
  clean, matching `fmt.yml` and `lint.yml` exactly.
- `scripts/check-enum-ordering.sh`, `scripts/check-enum-doc-comments.sh`,
  `scripts/check-workflow-version-pins.sh`, `scripts/regen-abis.sh` — all pass.

### Integration with checks that landed on `main` during this branch

This branch was rebased onto the then-current `main`, which had since gained a
`clippy-pedantic` job, `scripts/check-event-schema-drift.py`,
`scripts/check-error-code-uniqueness.sh`, and a rewritten deployment runbook. The
final commit resolves everything the rebase surfaced: `require_admin` fails closed
without `unwrap()`, the two new events are documented in `docs/event-schema.md`,
`initialize`/`transfer_admin`/`accept_admin` are added to
`docs/access-control-matrix.md`, and the runbook's `initialize` invocation now
passes `--admin`.

## Review notes

- `initialize`'s signature changed. This is a breaking interface change for
  anything calling it; both in-repo deploy paths are updated, and the ABI snapshot
  reflects the new entrypoints. Existing deployed workflow instances are
  unaffected (the pinned IDs are still read from the same `DataKey` variants) but
  would have no `Admin` set, so a redeploy is needed to use rotation.
- `accept_admin` returns `TreasuryError::NoPendingAdmin` rather than panicking
  with a string, matching the invoice contract's behaviour.

## Pre-existing problems found, not fixed here

Three things are broken on `main` today and are out of scope for these four
issues. Flagging them so they are not mistaken for regressions from this branch:

1. **`contracts/invoice/fuzz` and `contracts/settlement-workflow/fuzz` do not
   compile.** Both lack a lockfile, so their standalone workspace resolves
   `ed25519-dalek` 3.x, which does not satisfy `soroban-env-host`'s
   `ChaCha20Rng: CryptoRng` bound. The new compliance fuzz target is fixed by
   shipping a root-derived `Cargo.lock`; applying the same fix to the other two
   would make the scheduled `fuzz.yml` job able to run. That job's matrix is also
   hardcoded to those two targets, so the new compliance target is not yet
   exercised in CI — worth adding, but it edits a file another contributor just
   added.
2. **`clippy-pedantic` is red on `main`** — 16 findings across
   `compliance/src/lib.rs` (10), `settlement-workflow/src/lib.rs` (2),
   `treasury/src/lib.rs` (1) and two `invoice` files. This branch adds none; it
   fixes the one its own new code would have introduced.
3. **`abis/compliance.json` is stale on `main`** — `get_operator` (merged in #650)
   is missing from the snapshot. Not regenerated here to keep this diff focused,
   and `abi-drift-check.yml` only checks invoice so it is not currently gating.
4. `treasury` is the only contract with no `transfer_admin`/`accept_admin`, so its
   admin key is not recoverable without a redeploy. Noted in the deployment
   runbook; a rotation issue for treasury is probably warranted.

Closes #613
Closes #620
Closes #621
Closes #623
