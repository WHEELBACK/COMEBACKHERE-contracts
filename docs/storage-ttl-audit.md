# Storage TTL / Rent-Bump Audit

**Scope:** every persistent storage `DataKey` variant across the four contracts in
`contracts/*/src/**`. **Goal:** determine, per key, whether anything in this repo
ever calls `extend_ttl` (or the client-facing `bump_ttl` equivalent) against it, and
flag any key whose TTL is never explicitly extended as a genuine expiry risk.

## Why this matters

Soroban persistent (and instance/temporary) storage entries carry a live-until
ledger sequence. Once the current ledger sequence passes that number, the entry is
*archived*, not deleted: it still logically exists, but reading or writing it
requires an explicit (and non-trivial, off-chain-assisted) `RestoreFootprint`
operation before the contract can touch it again. A contract call that references
an archived key without first restoring it simply fails. Nothing in the contract's
business logic distinguishes "still relevant" from "long since touched" — the
network only tracks wall-clock/ledger time since the last bump, independent of
whether a human would consider the record active.

## Method

```sh
grep -rn "extend_ttl\|bump_ttl" contracts/*/src
```

Historically this returned **zero matches** anywhere in the four contract crates.
That is no longer true: `contracts/oracle-adapter` now routes every persistent
storage write through a shared TTL-bump helper (see "Oracle-adapter automatic TTL
bumps" below), and the invoice batch entrypoints extend TTL on the invoices they
write (see "Invoice batch entrypoints (#713)"). The rest of this document
catalogs every persistent key by contract, notes read/write frequency
characteristics that would determine natural TTL bumps, and calls out where the
absence of a bump is a genuine gap versus effectively harmless.
At the time this audit was first written this returned **zero matches** anywhere
in the four contract crates. That is no longer true: `access-control` now bumps
the TTL of every persistent key it writes (and of critical keys it reads) — see
"Access-control (`contracts/access-control/src`)" below. The remaining contracts
still call `extend_ttl` nowhere. The rest of this document catalogs every
persistent key by contract, notes read/write frequency characteristics that would
determine natural TTL bumps if any existed, and calls out where that absence is a
genuine gap versus effectively harmless.

Instance storage (`env.storage().instance()`) shares a single TTL for the whole
contract instance and is out of scope here except where noted — the interesting
risk is per-record archival of `persistent()` keys, where individual records can
go dark independently of each other and independently of the contract's own
liveness.

## Treasury (`contracts/treasury/src`)

`DataKey` is defined in `crates/multisig/src/lib.rs:124-142` and shared by the
treasury contract. Persistent keys used by treasury:

| Key | File / line | Written | Read | TTL bump anywhere? |
|---|---|---|---|---|
| `DataKey::Settlement(u64)` | `settlements.rs:119,179,361`; `disputes.rs:32,38,94,101,146,174` | `propose_settlement`, `hold_settlement`, dispute lifecycle | `execute_settlement`, `get_settlement`, dispute lifecycle | **No** |
| `DataKey::Dispute(u64)` | `disputes.rs:63,90,140,160,227` | `raise_dispute`, `resolve_dispute`, `vote_dispute_resolution` | `resolve_dispute`'s open-dispute scan, `get_dispute` | **No** |

**Finding — genuine gap:** a `Settlement` entry that enters `Pending` and then
sits waiting on quorum (nothing re-approves it, nothing disputes it, nothing calls
`execute_settlement`) receives **no write and no TTL bump** for as long as it
stays untouched. If enough real ledger time passes — which, per Soroban's default
minimum persistent TTL, is on the order of weeks-to-months depending on network
configuration, not years — the entry archives. The next `approve_settlement` or
`execute_settlement` call against that specific `settlement_id` would then fail
until someone restores the footprint, even though the settlement is still
logically "pending" from the business logic's point of view. This is exactly the
scenario the parent issue names. **Filed as a follow-up:** see
"Follow-up issues" below (Treasury Settlement TTL gap).

`Dispute` entries have the same property: a `Raised` dispute with a far-future
`dispute_expires_at` and no votes cast receives no bump either. In practice
disputes are expected to resolve or expire on a much shorter human timescale than
settlements can remain genuinely pending, so this is a smaller window of exposure,
but the mechanism is identical and worth carrying in the same follow-up.

## Access-control (`contracts/access-control/src`)

**Finding — remediated.** Persistent keys used by access-control:

| Key | Written by | Read by | TTL bump? |
|---|---|---|---|
| `DataKey::Admin` | `initialize`, `transfer_admin` | `require_admin` on every admin-gated call | **Yes** |
| `DataKey::Role(Address, Symbol)` | `grant_role` | `has_role`, `require_role` | **Yes** |
| `DataKey::RoleMembers(Symbol)` | `grant_role`, `revoke_role` | `get_role_members` | **Yes** |
| `DataKey::RoleCount(Symbol)` | `grant_role`, `revoke_role` | `get_role_member_count` | **Yes** |
| `DataKey::PendingAdmin` | `transfer_admin`, `accept_admin`, `cancel_transfer` | `accept_admin`, `cancel_transfer` | **Yes** |

**Policy.** Two constants in `contracts/access-control/src/lib.rs` define the
window:

- `TTL_THRESHOLD` — the remaining-TTL watermark below which a bump is triggered.
- `TTL_EXTEND_TO` — the TTL the entry is extended to when bumped
  (`TTL_EXTEND_TO > TTL_THRESHOLD`).

A single helper, `bump_persistent(env, &key)`, calls
`env.storage().persistent().extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO)`.
It is invoked:

- immediately after every `persistent().set(...)` of an access-control key, so
  any write refreshes the entry's lease; and
- on the read paths of the critical keys (`Admin`, `Role`, `RoleMembers`,
  `RoleCount`) so that a long-lived role or admin record that is only ever read
  still has its TTL refreshed before it can archive.

This closes the availability risk for access-control: an admin or role grant
that is never re-written but is still consulted on every gated call no longer
archives out from under the contract. The unit tests in
`contracts/access-control/src/test.rs` advance the ledger sequence with
`env.ledger().set_sequence_number(...)` past the threshold and assert the
entries remain readable.

## Compliance (`contracts/compliance/src`)

Persistent keys (all defined in `contracts/compliance/src/lib.rs:35-55`):

| Key | Written by | Read by | TTL bump anywhere? |
|---|---|---|---|
| `DataKey::Allowed(Address)` | `allow_address`, `allow_address_until`, `allow_address_with_tier`, `clear_address` | `is_allowed` | **No** |
| `DataKey::Blocked(Address)` | `block_address`, `block_address_until`, `clear_address` | `is_allowed`, `is_blocked` | **No** |
| `DataKey::AllowedUntil(Address)` | `allow_address_until`, `allow_address` (removes it) | `is_allowed` | **No** |
| `DataKey::BlockedUntil(Address)` | `block_address_until` | `is_allowed` | **No** |
| `DataKey::BlockReason(Address)` | `block_address`, `block_address_until` | (informational only) | **No** |

**Finding — genuine gap:** a `Blocked` entry set once and never revisited (the
address was flagged years ago, nobody ever calls `clear_address` or
`block_address` again for it) never gets its TTL extended either — every read via
`is_allowed`/`is_blocked` is a *read*, and Soroban does not extend TTL on read,
only on write or an explicit `extend_ttl` call. If that entry archives, the
*safety property silently flips*: `is_allowed` for that address, called against an
archived `Blocked` key, does not fail closed — the call would trap (footprint not
present), which at least fails loudly rather than silently permitting a
previously-blocked address through. This is a denial-of-service / availability
risk (the compliance check for that specific address stops working at all) rather
than a silent bypass, but it is still an unplanned operational surprise. **Filed
as a follow-up:** see "Follow-up issues" below (Compliance Blocked-entry TTL gap).

The same reasoning applies symmetrically to long-lived permanent `Allowed`
entries with no `AllowedUntil` — an address allowlisted once and never
re-touched is equally exposed, just fails safe (an archived `Allowed` read traps,
so the address stops being treated as allowed, erring toward the more
conservative direction).

## Invoice (`contracts/invoice/src`)

Persistent keys (`contracts/invoice/src/invoice.rs:131-152`):

| Key | Written by | Read by | TTL bump anywhere? |
|---|---|---|---|
| `DataKey::Invoice(u64)` | `create_invoice`, `mark_paid`, `cancel_invoice`, `extend_expiry`, batch entrypoints | most entrypoints | **No** |
| `DataKey::MerchantNonce(Address, u64)` | `create_invoice` (nonce guard), batch create | replay-guard check | **No** |
| `DataKey::MerchantInvoiceIndex(Address, u64)` | `create_invoice` | `get_invoices_by_merchant` | **No** |
| `DataKey::InvoiceHistory(u64)` | status-transition helper (`lib.rs:61`) | audit/history reads | **No** |
| `DataKey::PendingIndex` | `lib.rs:30,34` | expiry enumeration | **No** |
| `DataKey::LastCreatedAt(Address)` | `create_invoice` cooldown check | cooldown check | **No** |
| `DataKey::RefundBreakdown(u64)` (#71) | `process_refund`, alongside the `Refunded` transition | `get_refund_breakdown` | **No** |
| `DataKey::PayerIndexCount(Address)` | `mark_paid` | `get_invoices_by_payer` | **No** |
| `DataKey::PayerIndexEntry(Address, u64)` | `mark_paid` | `get_invoices_by_payer` | **No** |

**Assessment — lower risk, not a genuine gap today.** Invoices are designed
around a bounded lifecycle (`expires_at`, with an admin-tunable grace window);
the intended operational pattern is that a `Pending` invoice either gets paid,
cancelled, or expires within a period far shorter than a realistic minimum
persistent TTL. Once an invoice reaches a terminal status (`Paid`, `Cancelled`,
`Expired`), archival is actually the *desired* end state — nobody needs to
touch that record again on-chain, and if it does archive, the fix is a one-time
explicit restore for historical/audit lookups, not a live operational path. This
is intentional-by-inaction, not a gap, but it should be written down (this
document) rather than left implicit.

## Settlement-workflow (`contracts/settlement-workflow/src`)

`DataKey` (`contracts/settlement-workflow/src/lib.rs:26-29`) has exactly two
variants, `ComplianceId` and `TreasuryId`, both **instance** storage, set once at
`initialize` and read on every `execute_with_compliance*` call. Every call that
uses this contract re-reads (but does not write) instance storage, and instance
storage TTL is a single contract-wide value rather than per-record — out of scope
for the per-record archival risk this audit is about. No persistent keys exist in
this contract. **No gap.**

## Oracle-adapter automatic TTL bumps (#929)

`contracts/oracle-adapter` previously wrote its persistent keys without ever
extending their TTL, leaving price feeds and configuration records exposed to
ledger eviction. Every persistent write now goes through a single helper that
bumps the key's TTL immediately after the write, so no persistent key can be
written without its live-until ledger being pushed forward.

The helper uses two constants defined alongside the storage keys:

| Constant | Value (ledgers) | Meaning |
|---|---|---|
| `PERSISTENT_TTL_THRESHOLD` | `518_400` (~30 days) | Bump only when the remaining TTL drops below this |
| `PERSISTENT_TTL_EXTEND_TO` | `1_036_800` (~60 days) | New live-until horizon after a bump |

Design notes:

- **Threshold-gated, not unconditional.** `extend_ttl` is only invoked when the
  current remaining TTL is below `PERSISTENT_TTL_THRESHOLD`, so repeated writes
  in a short window do not pay for redundant bumps and host metering stays flat.
- **Write-adjacent.** The bump happens in the same call as the write, so a key
  that is actively being updated can never drift toward archival.
- **Reads do not bump.** As with every Soroban contract, `is_allowed`-style read
  paths do not extend TTL; only writes (and explicit admin bumps) do.

### Zero-balance defensive behaviour (#930)

The TTL helper and the surrounding storage paths are written to tolerate
edge-case zero-balance states without panicking contract execution:

- A zero balance is treated as a valid state, not an error: arithmetic that
  would otherwise underflow is guarded, and the resulting state is persisted
  normally rather than trapping.
- The TTL bump is skipped (not attempted, not panicked on) when there is no
  entry to extend, e.g. a key that was just removed or never written.
- Any failure to extend TTL degrades to "no bump" rather than aborting the
  enclosing contract call, so a metering or footprint hiccup cannot turn a
  legitimate zero-balance write into a failed transaction.

Unit and fuzz tests in the oracle-adapter test suite cover both the bump path
(threshold crossing, repeated writes) and the zero-balance edge cases (zero
amounts, missing entries, repeated zero writes) to confirm no host metering
regression and no panics.
## Dispute-resolver (`contracts/dispute-resolver/src`)

The dispute lifecycle lives in the treasury crate's `disputes.rs` (the
`dispute-resolver` contract crate is not present in this repo; the dispute
entrypoints are implemented on the treasury contract). Persistent keys touched
by the dispute lifecycle:

| Key | Written by | Read by | TTL bump anywhere? |
|---|---|---|---|
| `DataKey::Dispute(u64)` | `raise_dispute`, `resolve_dispute`, `vote_dispute_resolution` | `resolve_dispute`'s open-dispute scan, `get_dispute` | **Yes** (#920) |
| `DataKey::SettlementDisputes(u64)` | `raise_dispute` | `resolve_dispute`'s open-dispute scan | **Yes** (#920) |
| `DataKey::Settlement(u64)` | `hold_settlement` (dispute path) | `raise_dispute`, `resolve_dispute`, `vote_dispute_resolution` | **Yes** (#920) |

**Resolved (#920):** every dispute-lifecycle path now bumps TTL on the
persistent keys it touches via a shared helper, using
`DISPUTE_TTL_THRESHOLD = 518_400` (~30 days) and
`DISPUTE_TTL_EXTEND_TO = 1_036_800` (~60 days) ledgers, both defined in
`contracts/treasury/src/disputes.rs`. The helper is applied on `raise_dispute`,
`expire_dispute`, `resolve_dispute`, `resolve_dispute_split`,
`vote_dispute_resolution`, and `get_dispute`, so an open dispute (or a
settlement held by one) can no longer archive purely from elapsed time.

## Summary table

| Contract | Persistent keys audited | TTL bump exists? | Verdict |
|---|---|---|---|
| Access-control | `Admin`, `Role`, `RoleMembers`, `RoleCount`, `PendingAdmin` | **Yes** | Remediated — writes and critical reads bump TTL |
| Treasury | `Settlement`, `Dispute` | No | **Genuine gap** — long-pending settlement/dispute can archive |
| Dispute-resolver (treasury `disputes.rs`) | `Dispute`, `SettlementDisputes`, `Settlement` | **Yes** (#920) | Resolved — dispute lifecycle bumps TTL on every touched key |
| Compliance | `Allowed`, `Blocked`, `AllowedUntil`, `BlockedUntil`, `BlockReason` | No | **Genuine gap** — stale `Blocked`/`Allowed` entries can archive |
| Invoice | `Invoice` and secondary indices | No | Acceptable — bounded lifecycle, archival is the intended terminal state |
| Settlement-workflow | none persistent | N/A | No gap |
| Oracle-adapter | price feed / config keys | Yes — automatic bump on every persistent write | **Resolved (#929)** — threshold-gated `extend_ttl`; zero-balance states handled defensively (#930) |

## Follow-up issues

Per the parent issue's instruction to split genuine findings into their own
issues rather than fixing them all silently in one PR, the following two
follow-ups should be filed and linked from the parent audit issue:

1. **Treasury: `Settlement`/`Dispute` entries have no TTL-extension policy** —
   a settlement stuck `Pending` waiting on quorum, or an open dispute nobody
   votes on, can archive purely from elapsed time. Proposed fix direction: call
   `extend_ttl` on the relevant `Settlement`/`Dispute` key inside
   `approve_settlement`, `hold_settlement`, and `vote_dispute_resolution` (i.e.
   any write-adjacent path that touches the record while it's still open), plus
   consider an explicit admin-callable `bump_settlement_ttl` / `bump_dispute_ttl`
   entrypoint for records that see no activity at all.
2. **Compliance: `Blocked`/`Allowed` entries have no TTL-extension policy** — a
   compliance record set once and never revisited archives silently; the next
   `is_allowed` call against it traps instead of returning a normal boolean.
   Proposed fix direction: an admin-callable `refresh_ttl(address)` entrypoint,
   or an automatic `extend_ttl` call inside `is_allowed`/`is_blocked` reads for
   addresses known to be long-lived block/allow entries (note: reads alone don't
   extend TTL by default, so this would require an explicit call even on the
   read path).

Both follow-ups are scoped as their own PRs; this audit's job is documentation
and detection, not remediation. (Access-control's equivalent gap was remediated
in-tree — see "Access-control (`contracts/access-control/src`)" above — and is
the reference implementation for the two follow-ups.)

## Invoice batch entrypoints (#713)

`batch_create_invoice` and `batch_expire` now extend the TTL of every
`DataKey::Invoice(id)` they write, using `INVOICE_TTL_THRESHOLD = 518_400`
(~30 days) and `INVOICE_TTL_EXTEND_TO = 1_036_800` (~60 days) ledgers, both
defined in `contracts/invoice/src/invoice.rs`.

## Storage-layout remediation (#965)

`contracts/storage-layout` previously had the same "no `extend_ttl` anywhere"
property as the rest of the repo. It now bumps TTL on **every** persistent
read and write through a single shared helper, so no persistent key can be
touched without its live-until ledger being pushed forward:

```rust
env.storage().persistent().extend_ttl(
    key,
    STORAGE_LAYOUT_TTL_THRESHOLD,
    STORAGE_LAYOUT_TTL_EXTEND_TO,
);
```

Constants (defined in `contracts/storage-layout/src/lib.rs`):

| Constant | Ledgers | Approx. wall clock |
|---|---|---|
| `STORAGE_LAYOUT_TTL_THRESHOLD` | `518_400` | ~30 days |
| `STORAGE_LAYOUT_TTL_EXTEND_TO` | `1_036_800` | ~60 days |

The threshold is the "bump only if below this" trigger; the extend-to value is
the new live-until ledger written when the bump fires. Because the helper is
invoked on the read path as well as the write path (reads alone do not extend
TTL by default), a storage-layout record that is only ever queried still gets
refreshed, closing the "set once, never revisited" archival window described
above for the other contracts.

**Test coverage:** the Soroban test harness in `contracts/storage-layout`
asserts that (a) a write leaves the entry's live-until ledger at
`STORAGE_LAYOUT_TTL_EXTEND_TO` past the current sequence, (b) a read of an
entry whose TTL has fallen below the threshold bumps it back up, and (c) a
read of an entry still above the threshold leaves its TTL untouched (no
needless footprint growth / metering regression). These run under the standard
`Env` test harness with no host metering regressions.

## Storage-layout error modularization (#964)

`contracts/storage-layout` previously returned ad-hoc `panic!`/`unwrap`
failures. It now exposes a single `StorageLayoutError` enum with **stable
discriminants** (explicit `#[repr(u32)]`-style values that must not be
renumbered, since clients decode them from the `ScError` payload):

| Variant | Discriminant | Meaning |
|---|---|---|
| `NotInitialized` | `1` | Contract used before `initialize` |
| `AlreadyInitialized` | `2` | `initialize` called twice |
| `Unauthorized` | `3` | Caller lacks the required role |
| `KeyNotFound` | `4` | Requested persistent key is absent |
| `InvalidKey` | `5` | Key failed validation (e.g. empty/oversized) |
| `InvalidValue` | `6` | Value failed validation before write |

Every fallible entrypoint validates its inputs up front and returns the
matching variant via `Result<_, StorageLayoutError>` rather than trapping, so
clients get a decodable, documented error code instead of an opaque host
failure. Discriminants are treated as part of the public ABI: new variants are
appended, never inserted, and existing values are never reused. Unit tests in
the storage-layout suite cover each variant's trigger path, and the error
mapping is exercised alongside the TTL tests above.
