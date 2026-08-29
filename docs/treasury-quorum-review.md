# Treasury quorum-unreachability review: does `remove_signer` / `set_signer` risk a stuck threshold?

> **Status:** Design review · Documentation only, no code changes accompany this file.
> **Related:** Issue #33 (static "threshold reachable given total signer weight at init"
> test), `contracts/treasury/src/signers.rs`, `contracts/treasury/src/lib.rs`.

## Question

Issue #33 added a test confirming the treasury's configured threshold is
reachable given the total signer weight registered **at the moment that test
runs**. That is a useful static check, but it says nothing about whether
quorum stays reachable over the contract's operating lifetime, as signers are
added, removed, or reweighted. This document analyzes whether `remove_signer`
(or `set_signer` reducing/zeroing a weight) can ever leave the stored
`Threshold` permanently unreachable by the remaining signer set — grounded in
the actual code, not a hypothetical.

## What the code actually does

**`initialize`** (`contracts/treasury/src/lib.rs:22-66`) requires
`threshold != 0` but performs **no check** that `threshold <= total signer
weight` at initialization. (Issue #33's test covers this scenario by
convention/practice, not by an in-contract invariant.)

**`update_threshold`** (`contracts/treasury/src/lib.rs:69-92`) *does* guard
against unreachability when the threshold itself is being changed:

```rust
let total_weight: u32 = Self::get_all_signers(env.clone())
    .iter()
    .map(|(_, weight)| weight)
    .sum();
if new_threshold > total_weight {
    return Err(TreasuryError::ThresholdUnreachable);
}
```

**`set_signer`** (`contracts/treasury/src/signers.rs:12-38`) sets a signer's
weight (0 deactivates and removes them from `SignerList`) and **never reads
or compares against `DataKey::Threshold`**.

**`remove_signer`** (`contracts/treasury/src/signers.rs:46-65`) removes the
signer's storage entry and prunes them from `SignerList`, and likewise
**never reads or compares against `DataKey::Threshold`**.

So: `ThresholdUnreachable` is enforced on exactly one write path
(`update_threshold` raising or setting the threshold) and on **zero** of the
write paths that shrink total signer weight (`remove_signer`, `set_signer`
with a reduced or zero weight). The threshold and the signer set are two
independently-mutable pieces of state with no invariant tying them together
after initialization.

## Confirmed risk

Yes — `remove_signer` (or `set_signer(weight=0)`, or `set_signer` with a
reduced weight) **can** drop total remaining signer weight below the stored
`Threshold`, with no contract-level check preventing it and no automatic
threshold reduction to compensate.

**Minimal reproduction (traced through the code, not run):**

1. `initialize(admin, threshold=3, signers=[(A, 2)])` → admin has weight 1 by
   default (`lib.rs:52`), A has weight 2. Total weight = 3, threshold = 3.
   Reachable.
2. `remove_signer(admin, A)` → A's entry is deleted, `SignerList` becomes
   `[admin]`. Total remaining weight = 1 (admin only). `Threshold` is still
   3, untouched by `remove_signer`.
3. Every subsequent call that requires meeting `Threshold` via
   `meets_threshold(weight, threshold)` (`crates/multisig/src/lib.rs:181`,
   `weight >= threshold`) — settlement execution
   (`settlements.rs:232,302`), dispute resolution (`disputes.rs:214`),
   and signer rotation execution (`signers.rs:167-172`) — is now
   **permanently unapprovable**, because the maximum possible approval
   weight (1, from `admin` alone) can never reach 3.

Critically, step 3 includes **`approve_signer_rotation`** — the mechanism
that would normally let the remaining signers correct the situation by
rotating in a new signer — meaning this is not just a degraded state but a
**terminal lock-out**. There is no admin override that bypasses
`meets_threshold` for rotation approval; `cancel_rotation` can cancel a
pending proposal but cannot execute one below quorum, and the only way to
lower `Threshold` is `update_threshold`, which itself is admin-gated but not
signer-gated — meaning `admin` alone, with a single call to
`update_threshold`, *can* recover the treasury, provided `admin` still holds
their role and the situation is noticed before it's needed.

This is not a bug in the sense of violating a stated invariant — no code
comment or doc currently promises that quorum stays reachable after
`remove_signer`. It is a **missing invariant**: the threshold and signer-set
configurations are allowed to drift apart from each other with nothing
enforcing they stay mutually consistent, exactly as this issue's description
anticipated.

## Realistic triggers

- **Legitimate offboarding:** an operator rotates out a departing signer via
  `remove_signer` without first checking (or being prompted to check) whether
  the remaining weight still clears `Threshold`. Nothing in `remove_signer`'s
  interface signals this risk to the caller — no return value, no warning
  event distinguishing "safe removal" from "removal that drops quorum below
  threshold."
- **Compromised-key response:** an operator deliberately zeroes out a
  compromised signer's weight via `set_signer(signer, 0)` as an emergency
  measure. This is exactly the scenario where a rushed, high-pressure
  operational response is least likely to also re-derive and apply the
  correct new threshold in the same breath — and where getting it wrong is
  most consequential, since it's also the scenario most likely to be
  followed by disputes or contested settlements that need quorum to resolve.
- **Repeated attrition without rebalancing:** several small, individually
  reasonable `remove_signer` calls over time (staff turnover, key rotation
  hygiene) each look safe in isolation but cumulatively erode total weight
  below `Threshold` with no single call flagged as the one that crossed the
  line.

## Recovery path today

`admin` calling `update_threshold` to lower `Threshold` to at or below the
remaining total weight is the only in-contract recovery path, and it works
(`update_threshold` only rejects *raising* the threshold above total weight;
lowering it is always permitted as long as `new_threshold != 0`). So the
failure mode is recoverable **if and only if**:

- `admin` is still available and not itself part of the weight that was
  lost, and
- `admin`'s role was not also compromised/rotated away in whatever event
  caused the signer removal, and
- someone notices the lock-out and knows `update_threshold` is the fix,
  before a time-sensitive settlement or dispute resolution needs quorum.

That is a meaningfully weaker guarantee than "quorum is always reachable,"
and relies on a human noticing and acting rather than the contract enforcing
its own invariant.

## Recommendation

This is a real operational risk worth a follow-up code fix, not just a
documentation note. Two candidate mitigations, in order of preference:

1. **Guard `remove_signer` and `set_signer`** — before applying a weight
   reduction/removal, compute the resulting total signer weight and reject
   the call (return a new `TreasuryError` variant, e.g.
   `QuorumWouldBeUnreachable`) if it would drop below the stored `Threshold`.
   This mirrors `update_threshold`'s existing `ThresholdUnreachable` check,
   just applied from the other direction. Trade-off: this can itself become
   a lock-out vector in the opposite direction (an admin *can't* remove a
   signer during an emergency because doing so would breach quorum, even
   when that's the correct emergency action) — so this guard likely needs an
   explicit override path (e.g. requiring `update_threshold` to be called
   atomically alongside signer removal, or a combined
   `remove_signer_and_set_threshold` entrypoint) rather than an unconditional
   rejection.
2. **Auto-lower threshold on removal** — have `remove_signer`/`set_signer`
   automatically clamp `Threshold` down to `min(Threshold, new_total_weight)`
   when a reduction would otherwise breach quorum. Simpler to implement and
   never blocks a legitimate removal, but silently changes a
   protocol-critical security parameter (`Threshold`) as a side effect of an
   unrelated call, which is its own reviewability concern — every
   `remove_signer` call would need an accompanying `threshold_auto_lowered`
   event so this isn't silent.

Either option needs a new `#[repr(u32)]` error variant appended (not
inserted) to `TreasuryError` per `scripts/check-enum-ordering.sh`'s
append-only policy (see [`docs/pr-review-checklist.md`](./pr-review-checklist.md#2-repru32-enum-ordering-append-only)),
and a corresponding ABI snapshot regeneration.

## Suggested follow-up issue

A follow-up issue should be filed by a maintainer with GitHub write access;
this review does not open one. Suggested content:

> **Title:** `fix(treasury): prevent remove_signer/set_signer from dropping total weight below Threshold`
>
> **Body:** `docs/treasury-quorum-review.md` confirms `remove_signer` and
> `set_signer` (weight reduction/zeroing) have no check against the stored
> `Threshold`, unlike `update_threshold`, which does. This can permanently
> lock a treasury instance out of settlement execution, dispute resolution,
> and signer rotation if enough weight is removed. Implement mitigation
> option 1 or 2 from the linked review (guard vs. auto-lower), including a
> new append-only `TreasuryError` variant and updated ABI snapshot.
> References the economic review that identified this: this file.
