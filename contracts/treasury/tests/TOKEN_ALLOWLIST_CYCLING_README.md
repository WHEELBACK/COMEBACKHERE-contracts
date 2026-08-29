# Token allowlist cycling test — what was implemented

Issue: confirm `MAX_ALLOWED_TOKENS` on treasury's `TokenAllowlist` cannot be
bypassed by an admin rapidly removing and re-adding tokens near the cap
boundary (as opposed to only the straightforward monotonic add-until-full
sequences already covered elsewhere).

## File added

`contracts/treasury/tests/token_allowlist_cycling_test.rs`

## What it does

1. Reads `add_allowed_token` / `remove_allowed_token` in
   `contracts/treasury/src/settlements.rs`. The cap check
   (`allowlist.len() >= MAX_ALLOWED_TOKENS`) is evaluated against the
   allowlist's current size, freshly read from instance storage, on every
   `add_allowed_token` call. `remove_allowed_token` writes the updated list
   back synchronously before returning. There is no batching, deferred
   check, or transaction-boundary behavior in either function.

2. Adds four tests:
   - `cap_is_never_exceeded_across_rapid_remove_readd_cycles` — fills the
     allowlist to the cap, then runs 200 rapid remove-then-add cycles,
     asserting the length invariant (`<= MAX_ALLOWED_TOKENS`) holds after
     *every single* mutating call, not just at cycle boundaries.
   - `adding_at_cap_without_removing_first_is_still_rejected_during_cycling`
     — confirms the guard genuinely blocks growth past the cap (not just
     that the cycling test happens to always remove first).
   - `readding_a_token_already_present_is_a_no_op_and_does_not_grow_past_cap`
     — confirms the `contains()` short-circuit doesn't create duplicate
     entries or bypass the cap check.
   - `removing_and_readding_the_same_token_many_times_keeps_len_stable` —
     100 cycles on a single token, asserting the length alternates cleanly
     between `MAX_ALLOWED_TOKENS - 1` and `MAX_ALLOWED_TOKENS`.

## Finding

**Verified correct — no bug found.** The cap is checked against the
allowlist's current size at the moment each token is added, and cannot be
exceeded via remove/re-add cycling.

## Not run

Per the task instructions, this change was written without running
`cargo test` / `cargo clippy` / `cargo fmt` locally. Those checks are listed
as outstanding follow-up in the PR description.
