# Operational Alerting Guide

Recommended alert conditions for anyone running compliance, invoice, or
treasury in production. Every condition below is detectable today from an
existing event or read entrypoint — none require new contract code. Error
variant and event names are the literal identifiers emitted on-chain, so an
alerting pipeline can match on them directly.

This guide assumes an off-chain watcher subscribed to contract events (see
`scripts/reference-indexer.py` for a minimal example) and/or polling read
entrypoints on an interval.

## Treasury (`contracts/treasury`)

| Condition | Detection | Recommended threshold |
|---|---|---|
| Contract paused unexpectedly | `treasury_paused` event (topic `Symbol("treasury_paused")`, data: admin `Address`) with no corresponding planned-maintenance window | Page immediately — settlements cannot execute while paused |
| Signer/caller repeatedly rejected as unauthorized | `TreasuryError::UnauthorizedSigner` (10) or `TreasuryError::Unauthorized` (9) panics, visible in failed-transaction results | Alert on any occurrence outside expected key-rotation activity — may indicate a misconfigured integration or compromised/rotated key still being used |
| Settlement stuck on hold | `settlement_held` event (`contracts/treasury/src/holds.rs`) with no matching `settlement_released` within your SLA window; cross-check via `get_settlement(settlement_id)` — status stays `OnHold` | Warn after 24h on hold, page after 72h |
| Dispute open longer than its own expiry window | `dispute_raised` carries `dispute_expires_at` (caller-supplied UNIX timestamp); poll `get_dispute(dispute_id)` and alert when `env.ledger().timestamp() > dispute.dispute_expires_at` and `status == DisputeStatus::Raised` still — i.e. `expire_dispute` is callable but hasn't been called yet | Warn as soon as expired-but-uncalled; page if it persists past 2x the dispute's own window |
| Approval weight trending toward `WeightOverflow` | `TreasuryError::WeightOverflow` (25) is raised by `checked_add` on a `u32` running total in `multisig::record_approval` / `batch_approve_settlements`; poll `get_all_signers()` and sum weights | Warn if the sum of all signer weights exceeds ~90% of `u32::MAX` (≈3.86B) — this should never happen under normal operation, so treat any approach as a signer-configuration bug, not organic growth |
| Settlement past its TTL but not yet expired on-chain | `SETTLEMENT_TTL` is 7 days (`contracts/treasury/src/settlements.rs`); poll `get_pending_settlements_page` and alert when `proposed_at + 7d < now` for any `Pending` settlement | Warn — `expire_settlement` is admin-gated and won't fire itself |
| Pending settlement backlog growing | `get_pending_metrics()` → `(count, total_value)` (single-call aggregate, avoids paginating `get_pending_settlements_page`) | Alert on operator-defined count/value thresholds tuned to normal throughput |
| Treasury balance drained | `treasury_drained` event (`contracts/treasury/src/deposits.rs`) | Page immediately — this is an admin-only emergency-withdrawal path |
| Token removed mid-flight | `token_removed` event while settlements referencing that token are still `Pending` | Warn — those settlements will fail `execute_settlement` with `TreasuryError::TokenNotAllowed` (12) |

## Compliance (`contracts/compliance`)

| Condition | Detection | Recommended threshold |
|---|---|---|
| Contract paused unexpectedly | `compliance_paused` event with no planned-maintenance window | Page immediately — all compliance checks are blocked (`ContractError::ContractPaused` = 2) while paused |
| `AddressIndex` approaching capacity | `MAX_TRACKED_ADDRESSES` is `50_000` (`contracts/compliance/src/lib.rs`); once full, new-address tracking calls fail with `ContractError::AddressIndexFull` (5). Poll length via `export_snapshot(admin, 0, 0)` (or paginate with `export_snapshot_page`) | Warn at 80% (40,000 tracked addresses), page at 95% (47,500) — track growth rate, not just the instantaneous count, since this is a hard ceiling with no automatic eviction |
| `AddressIndexFull` actually returned | Any call failing with `ContractError::AddressIndexFull` (5) | Page immediately — this means new addresses can no longer be onboarded at all |
| Admin transfer left pending | `admin_transfer_initiated` event with no matching `admin_transferred` within your operational window | Warn — an unresolved pending-admin transfer is a standing privilege-escalation risk |
| Unexpected `address_blocked` volume | `address_blocked` event rate spike vs. baseline | Warn — may indicate a misbehaving upstream risk feed rather than genuine bad actors |

## Invoice (`contracts/invoice`)

| Condition | Detection | Recommended threshold |
|---|---|---|
| Contract paused unexpectedly | `contract_paused` event (`contracts/invoice/src/events.rs`) with no planned-maintenance window | Page immediately — `InvoiceError::ContractPaused` (2) blocks all mutating calls |
| Refund requests piling up unresolved | `invoice_refund_requested` events with no matching `refund_approved`/`refund_rejected` | Warn after 24h unresolved |
| Invoices expiring instead of being paid at unusual rate | `invoice_expired` event rate vs. `invoice_paid` rate | Warn on a sustained inversion (more expirations than payments) — may indicate a broken payment integration upstream |
| `DuplicateNonce` / `CooldownActive` rejections spiking | `InvoiceError::DuplicateNonce` (13) or `InvoiceError::CooldownActive` (21) | Warn — usually indicates a retrying client bug rather than a security issue, but worth tracking |

## General notes

- Prefer alerting on events over polling read entrypoints where an event
  exists for the condition — events are pushed and cheaper than repeated
  reads, and every `*_paused`/`*_unpaused` transition already emits one.
- Read entrypoints gated `admin`-only (`export_snapshot`, `export_snapshot_page`)
  require the monitoring identity to hold (or be authorized to call as) the
  contract's admin key; scope that key narrowly if it's only used for
  read-only monitoring.
- None of the thresholds above are protocol-enforced — they're operational
  judgment calls. Revisit them once real production traffic patterns exist.
