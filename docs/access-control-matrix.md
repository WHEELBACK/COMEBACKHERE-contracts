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
| `get_invoice_summary` | none | Permissionless (read-only) |
| `get_invoice_count` | none | Permissionless (read-only) |
| `get_pending_ids` | none | Permissionless (read-only) |
| `cancel_invoice` | `caller.require_auth()`, then in-body check `caller == invoice.merchant \|\| caller == admin` | Self-auth + ownership check |
| `amend_invoice` | `merchant.require_auth()`, then in-body check `invoice.merchant == merchant` | Self-auth + ownership check |
| `request_refund` | `payer.require_auth()`, then in-body check `invoice.payer == Some(payer)` | Self-auth + ownership check |
| `approve_refund` | `require_admin(&env, &admin)?` | Admin-only |
| `reject_refund` | `require_admin(&env, &admin)?` | Admin-only |
| `get_invoices_by_merchant` | none | Permissionless (read-only) |
| `get_invoices_by_payer` | none | Permissionless (read-only) |
| `batch_create_invoice` | `merchant.require_auth()` | Self-auth (role) |
| `batch_expire` | `require_admin(&env, &admin)?` | Admin-only |

## `contracts/treasury`

Source: `src/lib.rs`, `src/admin.rs`, `src/deposits.rs`, `src/settlements.rs`, `src/disputes.rs`, `src/holds.rs`, `src/signers.rs`, `src/timelock.rs`.

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
| `get_all_balances` | none | Permissionless (read-only) |
| `withdraw_all` | `require_admin(&env, &admin)`, plus explicit `paused` check (panics `NotPaused` if not paused) | Admin-only |
| `propose_settlement` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `propose_settlement_with_token` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `propose_partial_settlement` | delegates to `propose_settlement` | Authorized signer |
| `approve_settlement` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `revoke_approval` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `batch_approve_settlements` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `approve_partial_settlement` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `execute_settlement` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `batch_execute_settlements` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `partially_execute_settlement` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `cancel_settlement` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `batch_cancel_settlements` | `require_admin(&env, &admin)` | Admin-only |
| `force_cancel_settlement` | `require_admin(&env, &admin)` | Admin-only |
| `get_pending_settlements` | none | Permissionless (read-only) |
| `get_pending_settlements_page` | none | Permissionless (read-only) |
| `get_pending_metrics` | none | Permissionless (read-only) |
| `get_settlement` | none | Permissionless (read-only) |
| `get_settlements_by_merchant` | none | Permissionless (read-only) |
| `get_approvals` | none | Permissionless (read-only) |
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
| `resolve_dispute_split` | `require_admin(&env, &admin)` | Admin-only |
| `vote_dispute_resolution` | `require_authorized_signer(&env, &signer)` | Authorized signer |
| `hold_settlement` | `require_admin(&env, &admin)` | Admin-only |
| `get_hold_reason` | none | Permissionless (read-only) |
| `get_hold_expiry` | none | Permissionless (read-only) |
| `release_hold` | `require_admin(&env, &admin)` | Admin-only |
| `set_signer` | `require_admin(&env, &admin)` | Admin-only |
| `set_signer_label` | `require_admin(&env, &admin)` — sets a human-readable label for a signer; no pause check (role management) | Admin-only |
| `remove_signer` | `require_admin(&env, &admin)` | Admin-only |
| `get_signer_weight` | none | Permissionless (read-only) |
| `get_all_signers` | none | Permissionless (read-only) |
| `get_signer_last_active` | none | Permissionless (read-only) |
| `propose_signer_rotation` | `require_authorized_signer(&env, &proposer)` | Authorized signer |
| `approve_signer_rotation` | `require_authorized_signer(&env, &approver)` | Authorized signer |
| `cancel_rotation` | `require_admin(&env, &admin)` | Admin-only |
| `set_withdrawal_limit` | `require_admin(&env, &admin)` | Admin-only |
| `get_withdrawal_limit` | none | Permissionless (read-only) |
| `propose_signer_change` | `require_admin(&env, &admin)` — proposes a timelocked signer/threshold change; executable only after `SIGNER_CHANGE_TIMELOCK_SECS` | Admin-only |
| `execute_signer_change` | `require_admin(&env, &admin)` — executes a timelocked proposal after `executable_at` has passed; traps `SignerChangeTooEarly` if called before the delay | Admin-only |
| `cancel_signer_change` | `require_admin(&env, &admin)` — cancels a `Pending` timelocked proposal; traps `SignerChangeAlreadyFinalised` if already executed or cancelled | Admin-only |
| `get_signer_change` | none | Permissionless (read-only) |

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
| `is_blocked_with_proof` | none — permissionless read that walks the Merkle proof against the stored root | Permissionless (read-only) |
| `allow_address` | `Self::require_admin(&env, &admin)?` + `require_not_paused` | Admin-only |
| `allow_address_with_tier` | `Self::require_admin(&env, &admin)?` + `require_not_paused` | Admin-only |
| `allow_address_until` | `Self::require_admin(&env, &admin)?` + `require_not_paused` | Admin-only |
| `revoke_allow` | `Self::require_admin(&env, &admin)?` + `require_not_paused` | Admin-only |
| `get_address_tier` | none | Permissionless (read-only) |
| `set_tier_limit` | `Self::require_admin(&env, &admin)?` — sets a per-tier settlement cap; no pause check | Admin-only |
| `get_tier_limit` | none | Permissionless (read-only) |
| `set_jurisdiction` | `Self::require_admin(&env, &admin)?` — attaches an ISO jurisdiction code to an address; no pause check | Admin-only |
| `get_jurisdiction` | none | Permissionless (read-only) |
| `bulk_block_addresses` | `Self::require_admin(&env, &admin)?` — **no** `require_not_paused` call. Records the admin as each block's placer (#604). | Admin-only (see discrepancy below) |
| `block_address` | `Self::require_admin_or_operator(&env, &caller)?` — no pause check (documented: "permitted while paused"). Records the caller in `DataKey::BlockedBy` (#604). | Admin or operator |
| `block_address_until` | `Self::require_admin_or_operator(&env, &caller)?` — no pause check. Records the caller in `DataKey::BlockedBy` (#604). | Admin or operator |
| `get_block_reason` | none | Permissionless (read-only) |
| `get_block_placer` | none | Permissionless (read-only) |
| `get_schema_version` | none | Permissionless (read-only) |
| `migrate` | `Self::require_admin(&env, &admin)?` — runs a storage migration to bump the schema version; permitted while paused | Admin-only |
| `transfer_admin` | `Self::require_admin(&env, &admin)?` | Admin-only |
| `cancel_admin_transfer` | `Self::require_admin(&env, &admin)?` — cancels a pending admin nomination before it is accepted | Admin-only |
| `accept_admin` | `new_admin.require_auth()`; checked against stored `PendingAdmin` | Self-auth (role) |
| `clear_address` | `Self::require_admin_or_operator(&env, &caller)?` — no pause check (documented: "permitted while paused"). An operator caller must additionally pass `require_operator_placed_block`, which refuses unless `DataKey::BlockedBy[address] == caller`; an admin caller clears any block (#604). | Admin, or operator for an operator-placed block only |
| `pause` | `Self::require_admin(&env, &admin)?` | Admin-only |
| `pause_with_reason` | `Self::require_admin(&env, &admin)?` — same as `pause` but also stores a `Symbol` reason code readable via `get_pause_reason` | Admin-only |
| `get_pause_reason` | none | Permissionless (read-only) |
| `unpause` | `Self::require_admin(&env, &admin)?` | Admin-only |
| `set_operator` | `Self::require_admin(&env, &admin)?` — assigns the legacy single-operator slot; see also `add_operator` for the multi-operator set | Admin-only |
| `get_operator` | none | Permissionless (read-only) |
| `add_operator` | `Self::require_admin(&env, &admin)?` — adds an address to the bounded multi-operator set (max `MAX_OPERATORS`); no pause check (role management) | Admin-only |
| `remove_operator` | `Self::require_admin(&env, &admin)?` — removes an address from the multi-operator set; no pause check | Admin-only |
| `get_operators` | none | Permissionless (read-only) |
| `set_blocklist_root` | `Self::require_admin(&env, &admin)?` — commits or rotates the Merkle root for the bulk blocklist; permitted while paused (same emergency policy as `block_address`) | Admin-only |
| `get_blocklist_root` | none | Permissionless (read-only) |
| `get_allow_expiry` | none | Permissionless (read-only) |
| `get_address_history` | none — returns the bounded status-change history for an address (#603) | Permissionless (read-only) |
| `sweep_expired` | `Self::require_admin(&env, &admin)?` — explicitly expires lapsed time-bound allows and emits `address_allow_expired` events; not pause-gated (bookkeeping) | Admin-only |
| `export_snapshot` | `Self::require_admin(&env, &admin).unwrap()` (panics rather than returning `Err` on failure) | Admin-only |
| `export_snapshot_page` | `Self::require_admin(&env, &admin).unwrap()` (panics rather than returning `Err` on failure) | Admin-only |
| `address_status` | `Self::require_admin_or_operator(&env, &caller)?` | Admin or operator |

## `contracts/settlement-workflow`

Source: `src/lib.rs`.

| Entrypoint | Auth check in source | Category |
|---|---|---|
| `initialize` | `admin.require_auth()` on the `admin` parameter, then stores it along with the pinned compliance/treasury IDs. Guarded against re-entry by an `AlreadyInitialized` panic when a compliance ID is already stored. | **Self-auth (role)** — one-shot; the caller must be the admin it installs |
| `transfer_admin` | `require_admin(...)`: the `admin` parameter must authenticate *and* match the stored `DataKey::Admin`. Nomination only — the role does not move until the nominee accepts. | **Admin-only** |
| `accept_admin` | `new_admin.require_auth()` on the `new_admin` parameter, plus an in-body check that it matches the stored `DataKey::PendingAdmin`. Returns `Unauthorized` on a mismatch and `NoPendingAdmin` when nothing is outstanding. | **Self-auth (role)** |
| `pause` | `require_admin(&env, &admin)` — circuit-breaker for `execute_with_compliance*`; sets `DataKey::Paused = true` | Admin-only |
| `unpause` | `require_admin(&env, &admin)` | Admin-only |
| `is_paused` | none | Permissionless (read-only) |
| `get_admin` | none — returns the current workflow admin; returns `Err(Unauthorized)` if never initialized | Permissionless (read-only) |
| `execute_with_compliance` | **none directly in this function.** It calls `Compliance::is_allowed` (no auth required by that call) and, if it passes, `Treasury::execute_settlement` using `env.current_contract_address()` as the signer. Soroban auto-authorizes a contract's own outgoing calls, and `Treasury::execute_settlement`'s `require_authorized_signer` check is satisfied purely because this contract's address was pre-registered as a treasury signer via `set_signer`. | **Permissionless (mutating)** — see discrepancy below |
| `execute_with_compliance_batch` | none directly; loops through the same compliance and treasury call path as `execute_with_compliance` for each settlement ID. | **Permissionless (mutating)** — same invariant as above |
| `initialize_emergency_pause` | `admin.require_auth()` on the `admin` parameter (plain auth, not `require_pause_admin`); rejects reconfiguration while `EmergencyPausedAt` is present. This is the one-time (re-configurable) setup for the emergency pause orchestrator. | Self-auth (role) — any address may install itself as pause admin on first call; reconfiguration requires the already-registered admin to call again |
| `emergency_pause_all` | `admin.require_auth()` + `require_pause_admin` (must match pinned `EmergencyPauseAdmin`). Fans out `pause(self)` to every configured target in order; all-or-nothing. | Admin-only (emergency pause admin) |
| `resume_all` | `admin.require_auth()` + `require_pause_admin`. Guards: `NotEmergencyPaused` if `EmergencyPausedAt` absent. Fans out `unpause(self)` in reverse order; all-or-nothing. Accepts an optional `min_delay_secs` parameter: if non-zero, traps with `Unauthorized` when fewer than `min_delay_secs` seconds have elapsed since `EmergencyPausedAt`. | Admin-only (emergency pause admin) |
| `get_emergency_pause_targets` | none | Permissionless (read-only) |
| `get_emergency_pause_admin` | none | Permissionless (read-only) |
| `get_emergency_paused_at` | none | Permissionless (read-only) |

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
