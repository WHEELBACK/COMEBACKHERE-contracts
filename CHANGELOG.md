# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

#### Storage
- Storage key collision prevention audit (#86). `docs/STORAGE_VERSIONING.md` documents the key namespacing conventions (one append-only `#[contracttype]` key enum per contract, complete composite keys, no raw symbol keys) and the audited key space; the issue's `PaymentKey`/`ConfigKey` targets are mapped to the key enums that exist in this repository. New `storage_key_uniqueness_test.rs` suites assert that every variant of the invoice, treasury, compliance and settlement-workflow key enums serializes to a distinct XDR representation, and `tests/tests/storage_key_namespace_test.rs` proves both that identically-named variants in different contracts cannot collide and that neighbouring variants keep separate slots in real host storage.
- `invoice::StatusTransition` is now re-exported from the crate root so the invoice's status-transition type is usable by off-chain tooling and integration tests.

#### Invoice Contract
- Refunds verify payment state and fail safely across the contract boundary (#70). `approve_refund`, `reject_refund` and the new `process_refund` all run the same `verify_payment_state` check — an invoice with no payment timestamp, no recorded payer, or an inconsistent `amount_usdc`/`gross_usdc` pair is refused with the new `PaymentStateInconsistent` error before any refund state is written. `process_refund` also executes the payout on-chain: it transfers the invoice's `amount_usdc` from the contract's own escrow balance to the payer recorded at `mark_paid`, through the invoice's `token_address` contract, using `try_transfer` so a paused, drained or non-token contract returns the new `RefundTransferFailed` error instead of aborting the call. Because the transfer happens before the status transition is persisted, a failed payout leaves the invoice in `RefundRequested` and retryable rather than falsely recorded as `Refunded`. `RefundTokenNotSet` is returned when an invoice was created without a `token_address`.

- Refund payouts deduct processing and network fees, with a full gross-vs-net breakdown (#71). `process_refund` now takes a merchant `fee_bps` and transfers the *net* amount rather than the gross, computed by a single `calculate_net_refund` function that the payout, the stored record and the published event all share. The fee model is `processing_fee = gross_amount * fee_bps / 10_000` (rounded down) plus a flat `network_fee` capped at the gross amount, with `net_amount` flooring at `0` — so a refund can never pay out more than was paid in, and can never go negative. A `fee_bps` above 10_000 is refused with the new `RefundFeeTooHigh` error *before* the token contract is called, so a bad fee policy cannot produce a partial or mispriced payout. The intermediate product is decomposed rather than formed, because `gross_amount * fee_bps` overflows even `u128` for large amounts. The applied `NetRefund` is returned by `process_refund`, persisted under the new `DataKey::RefundBreakdown(id)`, and published in the new `refund_processed` event, so an indexer can reconcile what the customer actually received without re-deriving the fee arithmetic. Two new read entrypoints support that: `calculate_net_refund` previews a quote before approval (deliberately not pause-gated, so an operator can still price what a paused contract owes its customers), and `get_refund_breakdown` reads back what was applied to a given invoice.

#### Treasury Contract
- Timelocked signer and threshold-configuration changes (#447). Admin calls to `set_signer`, `remove_signer`, and `update_threshold` can now be queued via `propose_signer_change` with a 24-hour delay enforced by `execute_signer_change`; any admin can cancel the queued change within that window via `cancel_signer_change`. This restores a meaningful reaction window against a single-compromised-admin-key scenario that the pre-existing immediate entrypoints lacked.

#### Tooling
- `scripts/check-enum-doc-comments.sh` (#446): new lint script that verifies every `#[contracterror]` and `#[contracttype]` enum has a `///` doc comment on the enum itself (not just its variants). Integrated into `.pre-commit-config.yaml`, `justfile`, and `Makefile` alongside the existing `check-enum-ordering.sh` step.
- All `#[contracterror]`/`#[contracttype]` enums across `contracts/` and `crates/` now carry enum-level `///` doc comments to satisfy the new lint and to provide a high-level summary before readers dive into individual variant descriptions.

#### Invoice Contract
- Core invoice lifecycle management (Pending, Paid, Released, Cancelled, Expired, RefundRequested).
- Merchant-supplied nonces for idempotency and duplicate prevention.
- Configurable grace window for payment validity after quote expiry.
- Admin entrypoints for manual payment marking and escrow release.
- Batch expiry utility for cleaning up expired pending invoices.
- USDC decimal precision guardrails and positive amount validation.

#### Treasury Contract
- Multi-signature settlement workflow with configurable approval thresholds.
- Support for full and partial settlement proposals and execution.
- Dispute management system with the ability to place settlements on hold.
- Signer rotation mechanism via multi-sig proposal and approval.
- Token allowlist for restricted settlement asset support.
- Merchant payout address management.
- Contract-level pause/unpause for emergency mitigation.

#### Compliance Contract
- Admin-managed allowlist and blocklist for address-level access control.
- Support for time-bound (expiring) allowlist entries.
- Two-step admin transfer process for secure ownership handover.
- Emergency policy allowing blocking/clearing addresses even while the contract is paused.

#### Tooling & Docs
- Local testnet initialization script (`scripts/init-contracts.sh`).
- Development environment toolchain verification script (`scripts/check-tools.sh`).
- Per-contract READMEs with entrypoint reference tables.
- Root README documentation for toolchain version pinning.

#### Settlement Workflow Contract
- Compliance-gated settlement execution (`execute_with_compliance`): the recommended entry point for compliance-gated settlements (per `ARCHITECTURE.md`). It checks `Compliance::is_allowed` before invoking `Treasury::execute_settlement` using its own address as the authorizing signer, so the compliance gate is enforced even though Treasury does not consult compliance itself.
- Clear precondition failure: if the workflow contract has not been registered as a Treasury signer via `Treasury::set_signer` for its own address, `execute_with_compliance` returns `TreasuryError::WorkflowNotRegisteredSigner` (added to the shared `TreasuryError` enum) instead of surfacing Treasury's generic `UnauthorizedSigner`, making the missing setup step obvious to first-time deployers (#370).
- Auditable execution history: `get_executed_settlement_ids_page(start, limit)` returns the settlement IDs executed through this workflow (in execution order, paginated to mirror `Treasury::get_pending_settlements_page`), so an operator can confirm every executed settlement passed the compliance gate and spot any executed directly against Treasury that bypassed it (#373).
