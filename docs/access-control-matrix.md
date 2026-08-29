# Access-Control Matrix

This document enumerates every `pub fn` in the `#[contractimpl]` blocks of
`contracts/invoice`, `contracts/treasury`, `contracts/compliance`, and
`contracts/settlement-workflow`, and records the **actual** authorization
check found in each function's body — not what a doc comment claims, since
doc comments can lag behind implementation (see #14/#55-style precedent).
It exists to support #117's planned external audit and to give an
auditor, new contributor, or reviewer a single place to check "who can call
this?" without reading all five source files.

Each entry was verified by reading the function body directly. Where a doc
comment's stated auth requirement did not match the code, it is called out
explicitly in [Discrepancies found](#discrepancies-found) rather than
silently reconciled.

## Legend

| Category | Meaning |
|---|---|
| **Admin-only** | `require_admin(...)` (or the contract-local equivalent) is called: caller must be the stored admin address. |
| **Admin or operator** | Caller must be the stored admin *or* the stored operator address. |
| **Authorized signer** | `require_authorized_signer(...)`: caller must authenticate and hold non-zero weight in the multisig signer registry. |
| **Self-auth (role)** | The function calls `<param>.require_auth()` on a specific parameter (e.g. `merchant`, `claimant`, `payer`, `to`, `from`, `new_admin`) — anyone can call it, but only *as* that address, which they must be able to sign for. |
| **Self-auth + ownership check** | Self-auth as above, plus an additional in-body check that the authenticated caller matches a stored value (e.g. the invoice's recorded merchant) before the mutation proceeds. |
| **Permissionless (read-only)** | No auth call anywhere in the function; it only reads storage. |
| **Permissionless (mutating)** | No auth call anywhere in the function, yet it mutates state or moves funds. Flagged for extra scrutiny. |

## `contracts/invoice`

Source: `src/entrypoints/admin.rs`, `src/entrypoints/lifecycle.rs`, `src/entrypoints/batch.rs`.

| Entrypoint | Auth check in source | Category |
|---|---|---|
| `initialize` | `admin.require_auth()`; only succeeds once (`AlreadyInitialized` guard) | Self-auth (bootstrap) |
| `set_grace_window` | `require_admin(&env, &admin)?` | Admin-only |
| `get_grace_window` | none | Permissionless (read-only) |
| `transfer_admin` | `require_admin(&env, &admin)?` | Admin-only |
| `accept_admin` | `new_admin.require_auth()`; checked against stored `PendingAdmin` | Self-auth (role) |
| `pause` | `require_admin(&env, &admin)?` | Admin-only |
| `unpause` | `require_admin(&env, &admin)?` | Admin-only |
| `extend_expiry` | `require_admin(&env, &admin)?` | Admin-only |
| `create_invoice` | `merchant.require_auth()` | Self-auth (role) |
| `mark_paid` | `require_admin(&env, &admin)?` | Admin-only |
| `release_escrow` | `require_admin(&env, &admin)?` | Admin-only |
| `get_invoice` | none | Permissionless (read-only) |
| `get_invoice_status` | none | Permissionless (read-only) |
| `batch_get_invoice_status` | none (delegates to `get_invoice_status`) | Permissionless (read-only) |
| `get_invoices_page` | none | Permissionless (read-only) |
| `get_invoice_count` | none | Permissionless (read-only) |
| `get_pending_ids` | none | Permissionless (read-only) |
| `cancel_invoice` | `caller.require_auth()`, then in-body check `caller == invoice.merchant \|\| caller == admin` | Self-auth + ownership check |
| `amend_invoice` | `merchant.require_auth()`, then in-body check `invoice.merchant == merchant` | Self-auth + ownership check |
| `request_refund` | `payer.require_auth()`, then in-body check `invoice.payer == Some(payer)` | Self-auth + ownership check |
| `approve_refund` | `require_admin(&env, &admin)?` | Admin-only |
| `reject_refund` | `require_admin(&env, &admin)?` | Admin-only |
| `get_invoices_by_merchant` | none | Permissionless (read-only) |
| `batch_create_invoice` | `merchant.require_auth()` | Self-auth (role) |
| `batch_expire` | `require_admin(&env, &admin)?` | Admin-only |

## `contracts/treasury`

Source: `src/lib.rs`, `src/deposits.rs`, `src/settlements.rs`, `src/disputes.rs`, `src/holds.rs`, `src/signers.rs`.

| Entrypoint | Auth check in source | Category |
|---|---|---|
| `initialize` | `admin.require_auth()`; only succeeds once (`AlreadyInitialized` guard) | Self-auth (bootstrap) |
| `update_threshold` | `require_admin(&env, &admin)` | Admin-only |
| `pause` | `require_admin(&env, &admin)` | Admin-only |
| `unpause` | `require_admin(&env, &admin)` | Admin-only |
| `deposit` | `from.require_auth()` | Self-auth (role) |
| `batch_deposit` | `from.require_auth()` | Self-auth (role) |
| `withdraw` | `to.require_auth()` | Self-auth (role) |
| `get_balance` | none | Permissionless (read-only) |
| `withdraw_all` | `require_admin(&env, &admin)`, plus explicit `paused` check (panics `NotPaused` if not paused) | Admin-only |
| `propose_settlement` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `propose_partial_settlement` | delegates to `propose_settlement` | Authorized signer |
| `approve_settlement` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `batch_approve_settlements` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `approve_partial_settlement` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `execute_settlement` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `partially_execute_settlement` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `cancel_settlement` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `batch_cancel_settlements` | `require_admin(&env, &admin)` | Admin-only |
| `get_pending_settlements` | none | Permissionless (read-only) |
| `get_pending_settlements_page` | none | Permissionless (read-only) |
| `get_settlement` | none | Permissionless (read-only) |
| `expire_settlement` | `require_admin(&env, &admin)` | Admin-only |
| `update_merchant_payout_address` | `merchant.require_auth()` | Self-auth (role) |
| `get_merchant_payout_address` | none | Permissionless (read-only) |
| `add_allowed_token` | `require_admin(&env, &admin)` | Admin-only |
| `remove_allowed_token` | `require_admin(&env, &admin)` | Admin-only |
| `get_allowed_tokens` | none | Permissionless (read-only) |
| `raise_dispute` | `claimant.require_auth()` | Self-auth (role) |
| `expire_dispute` | `require_admin(&env, &admin)` | Admin-only |
| `get_dispute` | none | Permissionless (read-only) |
| `resolve_dispute` | `require_admin(&env, &admin)` | Admin-only |
| `vote_dispute_resolution` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `hold_settlement` | `require_admin(&env, &admin)` | Admin-only |
| `get_hold_reason` | none | Permissionless (read-only) |
| `release_hold` | `require_admin(&env, &admin)` | Admin-only |
| `set_signer` | `require_admin(&env, &admin)` | Admin-only |
| `remove_signer` | `require_admin(&env, &admin)` | Admin-only |
| `get_signer_weight` | none | Permissionless (read-only) |
| `get_all_signers` | none | Permissionless (read-only) |
| `propose_signer_rotation` | `require_authorized_signer(&env, &proposer)` | Authorized signer |
| `approve_signer_rotation` | `require_authorized_signer(&env, &approver)` | Authorized signer |
| `cancel_rotation` | `require_admin(&env, &admin)` | Admin-only |

## `contracts/compliance`

Source: `src/lib.rs`. Private helpers (`require_admin`, `require_admin_or_operator`,
`require_not_paused`, `address_state`, `track_address`) are not `pub fn` and are
not separate contract entrypoints; they're listed here only where an
entrypoint calls them.

| Entrypoint | Auth check in source | Category |
|---|---|---|
| `initialize` | `admin.require_auth()`; only succeeds once (`AlreadyInitialized` guard) | Self-auth (bootstrap) |
| `bulk_allow_addresses` | `Self::require_admin(&env, &admin)?` + `require_not_paused` | Admin-only |
| `bulk_check_addresses` | none (delegates to `is_allowed` per address) | Permissionless (read-only) |
| `is_allowed` | none | Permissionless (read-only) |
| `is_blocked` | none | Permissionless (read-only) |
| `allow_address` | `Self::require_admin(&env, &admin)?` + `require_not_paused` | Admin-only |
| `allow_address_with_tier` | `Self::require_admin(&env, &admin)?` + `require_not_paused` | Admin-only |
| `get_address_tier` | none | Permissionless (read-only) |
| `bulk_block_addresses` | `Self::require_admin(&env, &admin)?` — **no** `require_not_paused` call | Admin-only (see discrepancy below) |
| `block_address` | `Self::require_admin(&env, &admin)?` — no pause check (documented: "permitted while paused") | Admin-only |
| `block_address_until` | `Self::require_admin(&env, &admin)?` — no pause check | Admin-only |
| `get_block_reason` | none | Permissionless (read-only) |
| `get_schema_version` | none | Permissionless (read-only) |
| `allow_address_until` | `Self::require_admin(&env, &admin)?` + `require_not_paused` | Admin-only |
| `transfer_admin` | `Self::require_admin(&env, &admin)?` | Admin-only |
| `accept_admin` | `new_admin.require_auth()`; checked against stored `PendingAdmin` | Self-auth (role) |
| `clear_address` | `Self::require_admin(&env, &admin)?` — no pause check (documented: "permitted while paused") | Admin-only |
| `revoke_allow` | `Self::require_admin(&env, &admin)?` + `require_not_paused` | Admin-only |
| `pause` | `Self::require_admin(&env, &admin)?` | Admin-only |
| `unpause` | `Self::require_admin(&env, &admin)?` | Admin-only |
| `set_operator` | `Self::require_admin(&env, &admin)?` | Admin-only |
| `get_allow_expiry` | none | Permissionless (read-only) |
| `sweep_expired` | `Self::require_admin(&env, &admin)?` | Admin-only |
| `export_snapshot` | `Self::require_admin(&env, &admin).unwrap()` (panics rather than returning `Err` on failure) | Admin-only |
| `export_snapshot_page` | `Self::require_admin(&env, &admin).unwrap()` (panics rather than returning `Err` on failure) | Admin-only |
| `address_status` | `Self::require_admin_or_operator(&env, &caller)?` | Admin or operator |

## `contracts/settlement-workflow`

Source: `src/lib.rs`.

| Entrypoint | Auth check in source | Category |
|---|---|---|
| `execute_with_compliance` | **none directly in this function.** It calls `Compliance::is_allowed` (no auth required by that call) and, if it passes, `Treasury::execute_settlement` using `env.current_contract_address()` as the signer. Soroban auto-authorizes a contract's own outgoing calls, and `Treasury::execute_settlement`'s `require_authorized_signer` check is satisfied purely because this contract's address was pre-registered as a treasury signer via `set_signer`. | **Permissionless (mutating)** — see discrepancy below |

## Discrepancies found

Two gaps surfaced while cross-checking source against doc comments / sibling
functions; both are flagged here rather than silently reconciled, per this
task's requirement.

1. **`compliance::bulk_block_addresses` has an undocumented pause-bypass
   inconsistent with its own sibling.** `bulk_allow_addresses` calls both
   `require_admin` and `require_not_paused`. `bulk_block_addresses` calls
   only `require_admin` — it has no doc comment at all, unlike
   `block_address`/`block_address_until`/`clear_address`, which are each
   explicitly documented as "permitted while paused (emergency policy)".
   `bulk_block_addresses` behaves the same way (pause-bypassing) but nothing
   states that's intentional. An auditor reading only doc comments would
   have no way to know this batch entrypoint also works while paused.

2. **`settlement-workflow::execute_with_compliance` has no auth check of its
   own, and its doc comment does not disclose this.** The function's doc
   comment describes *what* it does (gate `execute_settlement` behind
   `is_allowed`) but never states that literally anyone can call it — its
   entire security boundary is (a) the compliance contract's `is_allowed`
   answer being honest, and (b) this contract's own address already holding
   non-zero signer weight in treasury. This is the one entrypoint in this
   matrix with no auth check at all, relying entirely on other invariants
   to be safe, which is exactly the category of entrypoint this matrix was
   built to surface. See also the reentrancy test added in
   `contracts/settlement-workflow/tests/settlement_workflow_test.rs` for the
   resulting call-ordering behavior when the compliance contract it calls is
   malicious or compromised.
