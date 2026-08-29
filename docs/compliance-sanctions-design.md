# Compliance: External Sanctions-List Integration — Design

> **Status:** Design proposal · No behavioral contract changes accompany this PR.
> **Issue:** #453
> **Branch:** `docs/compliance-sanctions-oracle-design`

## 1. Problem

`compliance`'s allow/block state is entirely admin-managed: every address-level
decision requires an explicit `allow_address` / `block_address` (or their
batch/timed variants) call. There is no mechanism to react to an *external*
sanctions-list update (e.g. a new OFAC addition) without an admin manually
noticing it and manually blocking each newly-sanctioned address — which does
not scale and introduces unbounded delay between a real-world sanctions event
and this protocol reflecting it.

This document designs an oracle/attestation pattern for feeding external
sanctions-list updates into the existing `block_address` mechanism. It
deliberately does **not** implement real off-chain data-source integration —
that depends on data-source and legal decisions out of scope for a contract
design doc (which feed, which jurisdiction's list, licensing, legal liability
for false positives/negatives).

## 2. Trust model options considered

| Option | Description | Trade-off |
|---|---|---|
| **A. Reuse existing `Operator` role** | The already-defined but unused `DataKey::Operator` (see `address_status`'s `require_admin_or_operator`) is granted a new capability: submitting sanctions updates. | No new storage/role plumbing. But `Operator` is a single address today — a single sanctions feed key becomes a single point of compromise, same class of risk as the admin key it's meant to reduce reliance on. |
| **B. Dedicated multisig "attestor set"** | A new weighted-signer set (mirroring `treasury`'s `multisig` crate: `signer_weight`, `record_approval`, `meets_threshold`) requires N-of-M attestors to agree before a sanctions batch applies. | Matches the trust bar of an irreversible, high-consequence action (blocking funds). Adds a second signer registry to a contract that otherwise has none — real complexity cost. |
| **C. External oracle contract** | A separate on-chain contract (or existing bridge/oracle infra) is trusted as a single cross-contract caller, verified via `require_auth()` on the oracle's own contract address. | Cleanest separation of concerns; sanctions-feed logic lives outside `compliance`. Requires that oracle contract to exist and be independently trustworthy/audited — nothing in this repo provides one today. |

## 3. Recommendation

**Start with Option A (reuse `Operator`), with an explicit upgrade path to
Option B.** Rationale:

- `Operator` already exists in storage (`DataKey::Operator`) and already has a
  defined trust tier below `Admin` (see `address_status`'s
  `require_admin_or_operator`) — extending its authority to sanctions
  submission is additive, not a new primitive.
- The realistic near-term operator of a sanctions feed is a single automated
  service (e.g. an indexer polling OFAC/SDN and relaying diffs), not a
  committee — a single authorized submitter matches that shape today.
- Option B's multisig attestor set is the right answer once more than one
  external feed/party needs to co-sign updates, but building it now would be
  speculative: `compliance` has no other multisig surface, and introducing one
  purely for this feature before a second real attestor exists is scope creep
  the issue itself explicitly cautions against ("rather than fully
  implementing actual off-chain sanctions-list integration end to end").
- Option C is the eventual target if/when a shared cross-protocol oracle
  contract exists, but nothing in this workspace provides one, and building a
  bespoke oracle contract is a separate, larger design effort.

## 4. Proposed shape (for the follow-up implementation issue)

Entrypoints, all gated on `require_admin_or_operator` (i.e. callable by
`Admin` or the designated sanctions-feed `Operator`):

```rust
/// Applies a batch of sanctions-list updates in one call. Each entry blocks an
/// address with a structured `reason` tag (distinct from the free-text
/// `BlockReason` used by manual `block_address`) so blocks originating from an
/// external feed are distinguishable from admin-initiated blocks in
/// `export_snapshot` / indexers.
pub fn apply_sanctions_update(
    env: Env,
    caller: Address,        // admin or operator
    addresses: Vec<Address>,
    list_id: Symbol,        // e.g. "OFAC_SDN"; identifies the source feed
    effective_at: u64,      // ledger timestamp the update should be attributed to
) -> Result<(), ContractError>;
```

Design notes carried over from the existing bulk entrypoints:

- Reuse `MAX_BATCH_SIZE` and `MAX_TRACKED_ADDRESSES` guards (#8/#21/#29, #48) —
  a sanctions batch is not exempt from the storage-growth bound.
- Reuse the `BULK_OP_COOLDOWN_SECS` velocity cap added for `bulk_block_addresses`
  (#454) — an external feed is not a more trusted caller than the admin key,
  and a compromised or malfunctioning feed should be bounded the same way.
- Emit a distinct event (`sanctions_block_applied`) rather than reusing
  `address_blocked`, so downstream indexers can separate "admin decided" from
  "external list said so" without parsing `BlockReason` free text.
- `list_id` is stored (new `DataKey::SanctionsSource(Address)`, analogous to
  `BlockReason(Address)`) so a later `clear_address` / dispute-style appeal
  process can show provenance.
- No automatic *unblock* path is proposed: removing an address from an
  external list should still require an explicit admin `clear_address` call —
  auto-unblocking on "the feed stopped listing it" is a materially different
  trust decision (a missing/failed feed poll must never silently unblock) and
  is out of scope here.

## 5. Threat model

- **Compromised operator key**: bounded by the existing `MAX_BATCH_SIZE` cap
  plus the reused `BULK_OP_COOLDOWN_SECS` velocity cap (#454) — same blast
  radius as a compromised admin key today, not worse.
- **Malicious/compromised feed pushing false positives**: the design
  intentionally keeps `block_address`'s existing semantics (an address can
  always be `clear_address`'d back by the real admin), and does not grant the
  operator any new *irreversible* capability — only `Admin` can permanently
  clear a block.
- **Stale/absent feed (false negatives)**: out of scope for this contract —
  detecting that a feed has stopped publishing is an off-chain monitoring
  concern, not something `compliance` can observe on its own.

## 6. Disposition

Per this issue's scope, this PR is **design-only**. The `apply_sanctions_update`
entrypoint and `DataKey::SanctionsSource` described in §4 are deferred to a
dedicated follow-up implementation issue, to be reviewed against this document
before any contract code is written — consistent with the issue's own
instruction to "propose the approach in the PR description and get it
reviewed before writing significant contract code."
