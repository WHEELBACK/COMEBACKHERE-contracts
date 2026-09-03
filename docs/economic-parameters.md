# Protocol-Wide Economic & Operational Parameters

This table collects every tunable economic/operational constant defined
across `contracts/*/src/**`, in one place, so that:

- an external auditor (per #117) can reason about how these values interact
  without cross-referencing four separate crates, and
- a future maintainer changing one value can see what else was chosen
  alongside it before doing so.

Each entry lists the constant's current value, its exact source location,
and a rationale. Where two constants were plausibly chosen with each other
in mind, that interaction is called out explicitly — this repo's inline
`///` doc comments document each constant on its own, but none of them
document cross-constant interactions, which is the gap this table exists
to close.

## Compliance (`contracts/compliance/src`)

| Constant | Value | Location | Rationale |
|---|---|---|---|
| `MAX_TRACKED_ADDRESSES` | `2_000` | `compliance/src/lib.rs` | Upper bound on the paged address index's growth. Once reached, tracking a *new* address is rejected with `AddressIndexFull` rather than growing the index further, to cap unbounded storage-rent growth. Existing tracked addresses are unaffected by the cap. |
| `MAX_BATCH_SIZE` | `50` | `lib.rs:132` | Cap on addresses accepted per admin batch call (`bulk_allow_addresses`, `bulk_block_addresses`). Chosen to match the identical `MAX_BATCH_SIZE = 50` used in `invoice` and `treasury` — see "Cross-contract interactions" below. |

## Invoice (`contracts/invoice/src`)

| Constant | Value | Location | Rationale |
|---|---|---|---|
| `USDC_FACTOR` | `10_000_000` (10^7) | `invoice.rs:4` | USDC on Stellar uses 7 decimal places (1 USDC = 10,000,000 stroops). Not tunable — this reflects the token's fixed on-chain precision, not a policy choice. |
| `MAX_BATCH_SIZE` | `50` | `invoice.rs:8` | Cap on elements accepted per call to `batch_create_invoice` / `batch_expire`-style entrypoints, to bound per-invocation storage writes and gas. Matches compliance's and treasury's `MAX_BATCH_SIZE`. |
| `MAX_BATCH_EXPIRE` | `100` | `invoice.rs:11` | Cap on invoice IDs accepted per call to `batch_expire` specifically. Set higher than the generic `MAX_BATCH_SIZE` because expiring an invoice is a cheaper storage operation (status flip) than creating one, so more can be processed per call within the same gas budget. |
| `MAX_HASH_BYTES` | `64` | `invoice.rs:14` | Cap on optional invoice hash fields (e.g. an off-chain document hash reference). 64 bytes comfortably covers SHA-512 (and any smaller digest) without allowing arbitrarily large blob storage on an unrelated field. |
| `MAX_EXPIRY_SECONDS` | `157_680_000` (5 years) | `validation.rs:5` | Maximum allowed invoice expiry duration. Bounds how long an invoice can remain `Pending` before it must resolve, preventing indefinitely-lived unpaid invoices from accumulating in storage. |
| Grace window | Admin-configurable, default `0` | `entrypoints/admin.rs` (`set_grace_window` / `get_grace_window`), applied in `entrypoints/lifecycle.rs` | Seconds added to `expires_at` when checking payment validity during `mark_paid` (#55). Deliberately *not* a compile-time constant — unlike the caps above, this is an admin-tunable knob because grace-period policy is expected to change post-deployment without a code upgrade. |

## Treasury (`contracts/treasury/src`)

| Constant | Value | Location | Rationale |
|---|---|---|---|
| `MAX_ALLOWED_TOKENS` | `20` | `lib.rs:116` | Cap on the token allowlist size, to prevent unbounded storage growth from an admin (or compromised admin key) adding tokens indefinitely. Read by `settlements.rs` when enforcing the allowlist. |
| `SETTLEMENT_TTL` | `604_800` (7 days) | `settlements.rs:9` | Time-to-live for a proposed settlement before it can no longer be executed. **Interacts with `MAX_BATCH_SIZE` below**: a 7-day window bounds how long a batch of up to 50 proposed settlements can sit awaiting multisig approval before expiring, which is the timing-pressure interaction an auditor should specifically check — a large batch proposed near the end of its TTL window compresses the effective approval window for every settlement in it. |
| `MAX_BATCH_SIZE` | `50` | `settlements.rs:13` | Cap on settlement IDs accepted per batch call. Matches invoice's and compliance's `MAX_BATCH_SIZE = 50` — chosen as a single workspace-wide default for "how many items can one call safely touch" rather than independently derived per contract (see #8/#21/#29 in the inline doc comments). |
| Signer rotation cooldown | `3_600` (1 hour) | `signers.rs:104` (`COOLDOWN_SECS`, local to `propose_signer_rotation`) | Minimum time between rotation proposals from the same proposer, to prevent rotation-proposal spam (e.g. from a compromised signer attempting to grind toward an unwanted rotation, or simple griefing). Not exposed as an admin-configurable value — changing it requires a code upgrade; see `docs/upgrade-guide.md`. |
| `threshold` | Set at `initialize`/`update_threshold`, no compile-time default | `lib.rs` (`initialize`, `update_threshold`) | Not a constant — enforced invariant is `0 < threshold <= total_weight` of registered signers. See `docs/deployment-runbook.md` §3 for how the actual mainnet value should be chosen relative to the weighted-quorum model. |

## Settlement-Workflow (`contracts/settlement-workflow/src`)

Settlement-workflow defines no numeric caps or windows of its own — it holds
only the pinned `ComplianceId` / `TreasuryId` addresses set once at
`initialize` (see `contracts/settlement-workflow/src/lib.rs`). All economic
parameters relevant to the settlement path live in treasury (above) and
compliance (below); settlement-workflow is a routing/gating layer over them.

## Cross-contract interactions worth flagging

- **`MAX_BATCH_SIZE = 50` is repeated identically in `compliance`, `invoice`,
  and `treasury`**, each as its own local `const` rather than a single
  shared constant. They were chosen together as one workspace-wide default
  (per the inline comments' references to #8/#21/#29), but nothing enforces
  that they stay in sync — changing one without the others would silently
  break the "one batch cap across the protocol" assumption an auditor might
  reasonably make from reading any single contract in isolation.
- **`SETTLEMENT_TTL` (7 days) vs. treasury's `MAX_BATCH_SIZE` (50)**: see the
  `SETTLEMENT_TTL` row above — a full batch of settlements proposed late in
  their TTL window has less real approval time than the nominal 7 days
  suggests.
- **Signer rotation cooldown (1 hour) vs. `SETTLEMENT_TTL` (7 days)**: these
  are on very different timescales by design — rotation cooldown limits
  proposal *spam* on a short horizon, while settlement TTL bounds
  settlement *liveness* on a much longer one. They do not directly interact,
  but both bound how quickly the signer set (rotation) or the pending-work
  queue (settlements) can be forced to move, which is relevant together when
  reasoning about worst-case time-to-recovery after a compromised signer is
  detected.

## Maintenance note

If you add or change a tunable constant in any contract, update this table
in the same change. This table is a manually maintained cross-reference, not
generated from source — it will drift if only the source-level `///` comment
is updated.
