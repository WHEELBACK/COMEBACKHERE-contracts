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

## Summary table

| Contract | Persistent keys audited | TTL bump exists? | Verdict |
|---|---|---|---|
| Treasury | `Settlement`, `Dispute` | No | **Genuine gap** — long-pending settlement/dispute can archive |
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
and detection, not remediation.

## Invoice batch entrypoints (#713)

`batch_create_invoice` and `batch_expire` now extend the TTL of every
`DataKey::Invoice(id)` they write, using `INVOICE_TTL_THRESHOLD = 518_400`
(~30 days) and `INVOICE_TTL_EXTEND_TO = 1_036_800` (~60 days) ledgers, both
defined in `contracts/invoice/src/invoice.rs`.
