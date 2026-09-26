# Storage Key Namespacing and Versioning

Status: living document. Last reviewed for issue #86 (storage key collision
prevention audit).

Every piece of contract state in COMEBACKHERE is addressed by a single
`#[contracttype]` enum per contract. This document states the conventions those
enums follow, why a collision in one of them corrupts state, and the procedure
for changing them without breaking already-deployed contracts.

## Why collisions are possible at all

Soroban derives a storage key's on-chain representation from the *value* of the
`#[contracttype]` enum, not from its Rust name. A unit variant becomes a
`Symbol`; a variant with fields becomes that symbol plus its fields, packed
into the key's XDR. Two consequences follow, and both are load-bearing:

1. **Within one contract, two variants that serialize identically share a
   slot.** If `Admin` and `Owner` ever produced the same XDR, the second write
   would silently overwrite the first and `get` would return whichever was
   written last. This is state corruption, not a failed call.
2. **Two variants never collide by accident of field layout.** The variant's
   own symbol is always the first element of the key, so `Invoice(1)` and
   `InvoiceHistory(1)` differ in their first element even though both carry the
   same `u64` and would be indistinguishable in a struct-based key scheme.
   Appending variants is therefore safe; reordering is not.

## Cross-contract isolation

Storage is namespaced by contract ID, so two different contracts may both have
an `Admin` variant without any risk to each other. This is asserted by
`tests/tests/storage_key_namespace_test.rs`, which writes through one contract's
`DataKey::Admin` and reads through another's. The consequence for reviewers: the
collision that matters is *always* within a single enum, which is why the
uniqueness tests are per-contract and why no global key registry is needed.

## Namespacing conventions

- **One key enum per contract, one concept per variant.** Every contract has
  exactly one `DataKey`; no contract mixes a `PaymentKey`/`ConfigKey`-style
  second enum into its key space. A contract with genuinely separable key
  families (a payment contract with both ledger and config keys) should keep
  them in one append-only enum and distinguish them by variant name, not by
  splitting the enum — two enums make the ordering guarantee twice as easy to
  break and hide half the key space from the audit.
- **Append only.** New variants go at the end, never in the middle, never
  reordered, never removed. Reordering changes the ordinal of every later
  variant and silently re-points existing stored data at a different logical
  field. This is enforced by `scripts/check-enum-ordering.sh` for error and
  type enums with explicit discriminants, and by review for `#[contracttype]`
  enums whose discriminants are implicit — the key-space enums are exactly the
  case where the script cannot help, so `tests/tests/storage_key_uniqueness_test.rs`
  and the per-contract uniqueness tests are the guard.
- **Composite keys carry every field that makes the record unique.**
  `DataKey::Balance(Address, Address)` in treasury is the reference example: an
  earlier single-address form would have merged balances of two concurrently
  allowlisted tokens into one accounting bucket.
- **Indexes get their own variant, never a prefix scan.**
  `DataKey::MerchantInvoiceIndex(Address, u64)` and `DataKey::AddrIndexPage(u32)`
  exist so enumeration is O(1)/O(page) rather than a full storage scan.
- **Deprecated keys are retained, never repurposed.**
  `DataKey::AddressIndex` in compliance is read by nothing and written by
  nothing; it is kept only so the enum stays append-only. Repurposing it would
  resurrect stale data under a new meaning.
- **No raw `Symbol`, `String` or ad-hoc tuple keys** for contract state. A raw
  symbol key has no compile-time relationship to the enum and is invisible to
  the audit these tests perform.

## Audited key space

| Contract | Key enum | Location | Variants |
|---|---|---|---|
| Invoice (escrow + refund records) | `invoice::DataKey` | `contracts/invoice/src/invoice.rs` | 14 |
| Treasury (escrow, settlements, signers) | `multisig::DataKey` (re-exported as `treasury::DataKey`) | `crates/multisig/src/lib.rs` | 24 |
| Compliance (allowlist/blocklist) | `compliance::DataKey` | `contracts/compliance/src/lib.rs` | 22 |
| Settlement workflow (orchestrator) | `settlement_workflow::DataKey` | `contracts/settlement-workflow/src/lib.rs` | 3 |

### Issue #86 audit targets and where they live

The issue asked for an audit of `enum DataKey`, `enum PaymentKey` and
`enum ConfigKey` across the payment, escrow and refund contracts. This
repository has no separate payment, escrow or refund contract — the concerns
are folded into the four contracts above — and it has no `PaymentKey` or
`ConfigKey` enum. The audit was therefore run over the key space that does
exist, mapped as follows:

| Issue target | This repository |
|---|---|
| payment contract keys | `invoice::DataKey` (invoices, escrow release) and `multisig::DataKey` (settlement payouts) |
| escrow contract keys | `multisig::DataKey` (`Settlement`, `Dispute`, `Balance`, payout addresses) |
| refund contract keys | `invoice::DataKey` (`RefundRequested`/`Refunded` transitions, `RefundBreakdown`) |
| payment/config key split | no split exists; both concerns live in one append-only `DataKey` per contract, per the convention above |
| orchestrator keys | `settlement_workflow::DataKey` |

The corresponding structural findings and fixes:

- No key enum mixes a raw key with a typed key, and no two variants within an
  enum serialize identically. Both are now asserted mechanically
  (`storage_key_uniqueness_test.rs` in each contract, plus the cross-contract
  isolation test in the `tests` crate).
- Each parameterized variant was checked for field completeness. The one
  historical defect found was treasury's `Balance(Address, Address)`, which
  originally keyed on the holder alone and merged per-token balances; the
  composite form is the audited, correct shape.
- Every contract exposes its enum publicly (`invoice::DataKey`,
  `treasury::DataKey`, `compliance::DataKey`, `settlement_workflow::DataKey`)
  so the audit is executable rather than a review comment.

## Change procedure

Adding a key:

1. Append the variant at the end of the contract's `DataKey`, with a `///`
   doc comment naming what it stores and which entrypoints touch it.
2. Add it to the exhaustive key list in that contract's
   `storage_key_uniqueness_test.rs`. A new variant that is not listed fails to
   be covered by review, since the test is what proves uniqueness.
3. If the variant is a new persistent key, extend `docs/storage-ttl-audit.md` in
   the same change.
4. Never renumber, reorder or remove an existing variant. Removing one strands
   its stored data, which still pays rent until it expires and still answers
   reads with the old value.
