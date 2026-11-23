# Signer rotation weight: fixed a time-of-check-to-time-of-use gap

## Problem

`approve_signer_rotation` read `old_signer`'s *current* approval weight at the
moment the rotation's approval threshold was met, then assigned that weight to
`new_signer`. If `old_signer`'s weight was changed by a separate `set_signer`
call, or the signer was removed via `remove_signer`, in a transaction landing
between `propose_signer_rotation` and the approval that finally crossed the
threshold, the weight assigned to `new_signer` depended on unrelated
administrative traffic racing the rotation rather than on the state at
proposal time.

## Fix

- `crates/multisig/src/lib.rs`: added `captured_old_weight: u32` to
  `SignerRotationProposal`.
- `contracts/treasury/src/signers.rs`:
  - `propose_signer_rotation` now snapshots `signer_weight(&env, &old_signer)`
    into `captured_old_weight` at proposal time.
  - `approve_signer_rotation` assigns `proposal.captured_old_weight` to
    `new_signer` on execution, instead of re-reading `old_signer`'s weight.
- `crates/multisig/Cargo.toml` version bumped `0.2.0` -> `0.3.0`, and
  `contracts/treasury/tests/multisig_version_lock_test.rs`'s ABI shape lock
  updated to match, per that file's own instructions for a field change.

## Tests added

`contracts/treasury/tests/rotation_weight_race_test.rs`:
- Baseline: no concurrent change, `new_signer` gets the weight captured at
  proposal time.
- `old_signer`'s weight is reduced by a `set_signer` call between proposal and
  execution — `new_signer` still gets the weight from proposal time.
- `old_signer` is removed entirely (`remove_signer`) between proposal and
  execution — `new_signer` still gets the non-zero weight from proposal time,
  not zero.
