# External Security Audit — Scope & Request for Proposal

> **Status:** Draft RFP · Coordination artifact · No code changes accompany this PR.
> **Issue:** Closes #308
> **Branch:** `docs/external-audit-scope`

This document scopes an external security audit of the COMEBACKHERE protocol
contracts. It is a coordination artifact prepared for circulation to candidate
auditors; it does **not** commit to any vendor, timeline, or budget. Scope is
derived **strictly** from `SECURITY.md`'s in-scope definitions and the
architecture described in `ARCHITECTURE.md`.

---

## 1. Purpose

Commission an independent, line-by-line review of the three in-scope contracts
by a third-party auditor experienced with Rust + Soroban/Stellar smart contracts.
The objective is to identify security weaknesses that internal reviews, unit
tests, and integration tests in this repository do not surface, with an emphasis
on **fund-safety** outcomes: loss, theft, lock-out, or unauthorized re-routing of
escrowed or settled assets.

This RFP defines:

- The code and vulnerability classes in scope
- The vulnerability classes and code explicitly **out** of scope
- Hotspots that warrant particular auditor attention
- The deliverables, methodology, and timeline we expect
- Vendor selection criteria

---

## 2. Scope — Vulnerability Classes

> **Authoritative source:** [`SECURITY.md`](../SECURITY.md). This section is a
> verbatim restatement for vendor convenience. **Any conflict between this
> document and `SECURITY.md` defaults to `SECURITY.md`.** Maintainers must
> re-confirm SECURITY.md is unchanged at the moment of vendor distribution.

The audit is confined to the four vulnerability classes declared in-scope by
[`SECURITY.md`](../SECURITY.md):

| # | Class (per SECURITY.md) | Audit lens |
|---|---|---|
| A | **Logic errors in contract state machines** (`invoice`, `treasury`, `compliance`) | State-transition coverage; terminal-state invariants; idempotency; concurrent/duplicate admin actions. |
| B | **Authorization bypass or privilege escalation** | `require_auth()` correctness, role-based access on every mutating entrypoint, admin/operator/signer separation, two-step admin/rotation procedures. |
| C | **Integer overflow/underflow in payment or settlement math** | All arithmetic on amounts, weights, thresholds, balances, counts, fees, indexes, timestamps, and pagination cursors. |
| D | **Reentrancy or cross-contract call vulnerabilities** | External calls to token contracts and between `invoice`/`treasury`/`compliance`/`settlement-workflow`; state-write ordering relative to external calls. |

Vendors must produce findings for **all four classes**, even when a class
produces zero findings (negative-result declarations are required).

---

## 3. Scope — Code

The three contracts declared in-scope by `SECURITY.md` and `ARCHITECTURE.md`.

### 3.1 Invoice — `contracts/invoice/`

Source root: `contracts/invoice/src/`

Entrypoints to audit (every public function declared via `#[contractimpl]`):

- `create_invoice`, `mark_paid`, `release_escrow`, `cancel_invoice`,
  `request_refund`, `approve_refund`, `batch_expire_invoices`,
  `update_invoice_amount`, `extend_invoice_expiry`, `get_invoice`,
  `batch_get_invoice_status`, admin/pagination helpers, and the `PendingIndex`
  maintenance paths exposed in `lib.rs`.

Risk hotspots to call out for this contract:

- **State machine A:** transitions across
  `Pending → Paid → Released / RefundRequested / Cancelled / Expired` and the
  interaction with the `PendingIndex` and `InvoiceHistory` append-only log; the
  grace-window (`GraceWindow`) extending `expires_at` during `mark_paid`.
- **Math C:** `InvoiceAmountUpdatedEvent` arithmetic, batch expiry cap
  (`MAX_BATCH_EXPIRE`), pagination cursor arithmetic in
  `batch_get_invoice_status`, `MAX_BATCH_SIZE` envelope.
- **Auth B:** admin-only mutation entrypoints; merchant-nonce
  (`MerchantNonce(Address, u64)`) idempotency collisions; the
  `cancel_invoice` / `approve_refund` authorization split; merchant-vs-payer
  separation on `mark_paid`.
- **Reentrancy D:** Invoice is documented as having **no outbound
  cross-contract calls** (per `ARCHITECTURE.md`); the auditor should still
  verify this and flag any future client/bounty program that would invalidate
  the guarantee.

### 3.2 Treasury — `contracts/treasury/`

Source root: `contracts/treasury/src/`
Multi-sig surface: `crates/multisig/src/`

Entrypoints to audit (every public function in `lib.rs`,
`signers.rs`, `settlements.rs`, `holds.rs`, `disputes.rs`, `deposits.rs`),
including:

- `initialize`, `pause`/`unpause`, `update_threshold`,
  signer lifecycle (`add_signer`, `remove_signer`, signer rotations), all
  settlement entrypoints (`propose_settlement`, full + partial approval flows,
  `execute_settlement`), deposit/withdraw, hold/release, dispute lifecycle,
  rotation (`propose_rotation`, `approve_rotation`, `execute_rotation`,
  `cancel_rotation`), `execute_settlement` token-transfer integration,
  `MerchantPayoutAddress` overrides, pagination helpers, and `get_pending_settlements_page`.

Risk hotspots to call out for this contract:

- **State machine A:** `SettlementStatus`, `DisputeStatus`, `RotationStatus`,
  `SettlementHoldReason` interactions; placement and release of holds relative
  to settlement execution and dispute lifecycle; cumulative-partial-approval
  accounting against settlement amount (rejected if cumulative would
  over-approve).
- **Math C:** `Threshold`, `Signer(Address)` weight sums at or above
  `u32::MAX`; `Balance(Address)` `i128` deposit bookkeeping; partial-settlement
  residue arithmetic; counter monotonicity (`SettlementCount`, `DisputeCount`,
  `RotationCount`); pagination cursor arithmetic; token-allowlist cap
  (`MAX_ALLOWED_TOKENS = 20`).
  - **Already-landed-but-still-residual-risk:** Cumulative partial approvals
    that would exceed the settlement amount are rejected by the current
    `main`. Auditor should confirm the fix has not regressed and report
    **only adjacent** residual risk (e.g., partial-then-cancel ordering,
    quantity-cast mismatch between proposer and approver).
- **Auth B:** `require_admin` / `require_signer` / `require_not_paused`
  ordering; signers must authorize before being counted; threshold-update and
  signer-rotation cooldowns (rotation cooldown between proposal and execution);
  merchant-payout override authorization; signer list uniqueness.
- **Reentrancy D:** **The** highest-risk cross-contract surface — `execute_settlement`
  calls the SEP-41 `Token::transfer` contract. Auditor must verify
  Checks-Effects-Interactions (CEI) ordering: state writes (status change,
  balance decrement, hold removal) occur **before** the token transfer call;
  and that any partial-transfer / rollback semantics do not leave treasury
  storage and token ledger inconsistent. The
  `crates/compliance-client/src/lib.rs` ergonomic wrapper that mediates
  `Compliance::is_allowed` is also in scope.

### 3.3 Compliance — `contracts/compliance/`

Source root: `contracts/compliance/src/`

Entrypoints to audit (every public function in `lib.rs`):

- `initialize`, `is_allowed`, `is_blocked`, `address_status`,
  `allow_address`, `allow_address_until`, `allow_address_with_tier`,
  `bulk_allow_addresses`, `bulk_check_addresses`, `bulk_block_addresses`,
  `block_address`, `block_address_until`, `clear_address`, `revoke_allow`,
  `transfer_admin` / `accept_admin`, `pause`/`unpause`, `set_operator`,
  `sweep_expired`, `export_snapshot` / `export_snapshot_page`,
  `get_address_tier`, `get_allow_expiry`, `get_block_reason`,
  `get_schema_version`.

Risk hotspots to call out for this contract:

- **State machine A:** `AddressIndex` membership uniqueness on
  `track_address`; `MAX_TRACKED_ADDRESSES` cap; lazy-expiry semantics of
  `AllowedUntil` and the discrete `address_allow_expired` event emitted by
  `sweep_expired`; the supersession-on-recall behavior of
  `transfer_admin` (no expiry on `PendingAdmin`).
- **Math C:** pagination cursor arithmetic in `export_snapshot` /
  `export_snapshot_page`; `AllowCount` / `BlockCount` saturating-sub
  underflow safety; tier (`u32`) handling; timestamp comparisons.
- **Auth B:** admin and operator separation (`require_admin_or_operator`);
  emergency policy that `block_address` / `clear_address` are **permitted
  while paused** — auditor must verify the pause does not unintentionally
  permit other privileged mutating entrypoints; super-admin transfer two-step
  completeness; and — **separately** — whether `bulk_block_addresses`
  silently bypasses `require_not_paused` is, in itself, a *finding* if
  inconsistent with the documented emergency policy applied to single-address
  `block_address`. Auditor must classify this as either (i) intentional
  emergency-policy parallelism (acceptable) or (ii) an inconsistency that
  deserves its own finding.
- **Reentrancy D:** compliance makes **no** outbound cross-contract calls;
  the auditor must confirm, and must validate the cross-contract **call
  surface** when `Compliance::is_allowed` is invoked via
  `crates/compliance-client/` and `contracts/settlement-workflow/`.

### 3.4 Cross-contract composition

- `contracts/settlement-workflow/` — `execute_with_compliance` composes
  `Compliance::is_allowed(merchant)` and `Treasury::execute_settlement`. The
  order in which the compliance check, the treasury `execute_settlement`
  token-transfer, and any state writes occur is **the** principal
  reentrancy / TOCTOU surface for the entire protocol.
- `crates/compliance-client/` — the ergonomic `ComplianceClient::require_allowed`
  wrapper used by callers.
- `crates/multisig/` — shared types the auditor must read to reason about
  treasury state-machine diagrams (`Settlement`, `Dispute`,
  `SignerRotationProposal`, `SettlementStatus`, `DisputeStatus`,
  `RotationStatus`, `SettlementHoldReason`).
- `crates/protocol-errors/` — the unified `ProtocolError` enum and the
  named `InvoiceError` / `TreasuryError` / `ComplianceError` variants;
  enumerates all documented denial paths an auditor should still try to
  bypass.

---

## 4. Out of Scope

Directly per `SECURITY.md`:

- Stellar/Soroban network protocol issues (route to the Stellar Bug Bounty).
- Vulnerabilities in upstream dependencies outside this repository.
- Pure-theoretical findings with no realistic exploit path against this
  codebase.

Additionally out of scope for **this** engagement:

- ABI snapshot / metadata correctness (handled by sibling `COMEBACKHERE/`).
- Any off-chain indexer, frontend, or business-process code outside this repo.
- Gas / fee optimization that does not affect security.
- Refactoring, documentation, or test-coverage suggestions unless they
  unmask a security weakness.

---

## 5. Areas Warranting Particular Auditor Attention — Fund-Safety Hotspots

The following items from this repository's own backlog are known fund-safety
risk areas. The auditor should treat each as a **prioritized** review focus and
explicitly affirm (with diff-level evidence) whether the current code resolves
the named concern.

> **Caption caveat for vendors:** The "Thematic area" captions in the table
> below are an administrative reading derived from on-chain architecture and
> the changelog, **not** verbatim issue titles from the issue tracker. The
> engineering owner must replace each caption with the verbatim tracker
> title (and any sub-tasks) before circulating this RFP; if a thematic
> caption proves inaccurate at issue-discovery time, the auditor should
> pivot to whatever the actual tracker text directs.

| Issue | Thematic area (administrative reading — confirm before issue) | Why this matters now |
|---|---|---|
| #28 | Invoice state-machine / escrow release | Reconciliation between invoice escrow release and treasury disbursement; release-of-unpaid-invoice risk. |
| #33 | Settlement amount / partial approval math | Cumulative-partial-approvals-over-amount risk in treasury settlement state machine. (See §3.2 Math C: a fix is already on `main`; verify and report any residual.) |
| #34 | Token-allowlist / payout-override safety | Token allowlist enforcement (`MAX_ALLOWED_TOKENS`) and `MerchantPayoutAddress` override authorization when payment is executed. |
| #36 | Threshold / signer-weight math | Edge cases in multisig weight accumulation, threshold of zero, signer-list uniqueness during rotation. |
| #41 | Compliance gate timing in cross-contract settlement | Whether the `SettlementWorkflow.execute_with_compliance` ordering grants a TOCTOU window between the compliance read and the treasury token transfer. |

For each of the above the auditor must report:

1. Whether the issue is **resolved**, **partially resolved**, or **open**, with
   line-level references.
2. Any **adjacent** issue surfaced by the same review (related-state-machine
   bug, latent reentrancy, related math edge case, etc.).
3. A concrete test that would demonstrate the issue (PoC contract call or unit
   test), regardless of whether the issue is open or not.

---

## 6. Methodology

The auditor is expected to apply, at minimum:

1. **Manual line-by-line review** of every `#[contractimpl]` function in the
   three in-scope contracts, plus every shared type relevant to state-machine
   reasoning (`crates/multisig`, `crates/protocol-errors`).
2. **Threat modeling** per the four SECURITY.md classes against the cross-contract
   arrows in `ARCHITECTURE.md`, treating `SettlementWorkflow` as the central
   adversary target.
3. **Adversarial test construction** — the auditor is encouraged to author
   reproducible failing tests (or transaction simulations) and submit them
   alongside findings, even when the same finding can also be presented as a
   static-analysis note.
4. **Differential review** of any module whose error enum enumerates a denial
   path, to confirm that no sequence of preceding writes allows the same
   operational outcome to be reached via a different code path.
5. **Coverage statement** — for each of the four SECURITY.md classes, the
   auditor must state which files/functions were reviewed and which were
   deemed out-of-methodology-scope (and why).

The auditor should also reproduce, on their own tooling, any reported PoC and
state the Soroban SDK and `soroban-sdk` version they used.

---

## 7. Deliverables

1. **Markdown audit report** committed (or delivered) into this repository
   under `docs/audits/<vendor>-<yyyymmdd>.md`, with TOC by severity.
2. **Findings table** with, per finding: severity (Critical/High/Medium/Low/Info),
   SECURITY.md class mapping (A/B/C/D), contract + file + line, description,
   impact, PoC or test, recommended fix, and status (Fixed/Open).
3. **Per-class attestation**: a one-paragraph statement per SECURITY.md class
   confirming reviewed code and any negative-result declaration.
4. **Reproduction harness**: minimal Soroban test crate or shell transcripts
   that, when re-run, reproduce every High and Critical finding.
5. **Suggested remediations** drafted against the current `main` (post-PR-merge
   state) so the maintainers can patch in-line.
6. **Report-walkthrough** call within 14 days of report delivery, with the
   engineering team, to triage findings.

---

## 8. Vendor Selection Criteria

Required:

- Demonstrated Rust + Soroban/Stellar-specific prior audit experience.
- Independent — no equity, employment, or commercial relationship with the
  COMEBACKHERE organization or its signers.
- Willingness to coordinate with the project's coordinated-disclosure timeline
  (`SECURITY.md`, 14 days critical / 30 days other) before any public write-up.

Preferred:

- Familiarity with SEP-41 (token), and with Soroban auth/storage/cross-call
  primitives.
- Prior experience auditing protocol-roots that interpose a compliance gate
  between an off-chain workflow contract and the on-chain settlement layer.
- Willingness to run a small paid pilot on **only** the `treasury` settlement
  arithmetic before committing to a full report.

---

## 9. Timeline

Aligned with `SECURITY.md` response SLAs where they apply to the engagement:

| Milestone | Target |
|---|---|
| RFP distributed to candidate auditors | Within 7 days of PR merge |
| Vendor selection | Within 21 days of RFP distribution |
| Kick-off + access to repo / test scaffolding | Within 14 days of selection |
| Interim findings call (Critical/High only) | Mid-engagement |
| Final report delivery | Within 6 weeks of kickoff |
| Report walkthrough + triage | Within 14 days of delivery |
| Coordinated disclosure window | Per `SECURITY.md` (14 days critical / 30 days other) |

Coordinated-disclosure embargo applies to all Critical and High findings
until the engineering team has shipped or scheduled a fix.

---

## 10. Coordination & Communication

- Primary contact: project maintainers (see PR reviewers on this RFP PR and
  the maintainers listed in `SECURITY.md`'s reporting channel).
- Daily standup on weekdays during the active engagement; ad-hoc escalation
  channel for Critical findings.
- All report artefacts (raw notes, scripts, PoC contracts) to be delivered into
  this repository under `docs/audits/` before public release.
- Coordinated disclosure per `SECURITY.md`: findings may not be published
  publicly before the engineering team's patch is released and credited.

---

## 11. References

- [`SECURITY.md`](../SECURITY.md) — vulnerability classes in/out of scope,
  response SLAs, disclosure policy. **Authoritative for scope.**
- [`ARCHITECTURE.md`](../ARCHITECTURE.md) — payment lifecycle, cross-contract
  call map, failure-mode table, invoice state machine.
- [`README.md`](../README.md) — workspace layout, toolchain pins.
- `crates/multisig/src/lib.rs` — shared treasury types.
- `crates/protocol-errors/src/lib.rs` — unified `ProtocolError`, all denial
  paths the auditor may try to bypass.
- `crates/compliance-client/src/lib.rs` — cross-contract `Compliance` wrapper.
- `contracts/settlement-workflow/src/lib.rs` — composition of compliance +
  treasury at the heart of cross-contract risk.
- Issues **#28, #33, #34, #36, #41** — fund-safety hotspots enumerated in §5.

---

## 12. Change Log for This Document

| Version | Author | Note |
|---|---|---|
| v1 | this PR | Initial RFP, scoped strictly to SECURITY.md in-scope definitions + the named fund-safety backlog issues. |
