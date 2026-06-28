# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
