# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
