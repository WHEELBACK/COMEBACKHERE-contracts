# Mainnet / Testnet Deployment Runbook

`scripts/init-contracts.sh` deploys and initializes all four contracts on a
disposable local network for day-to-day development. It is a good reference
for the *mechanics* of deployment, but it deliberately skips everything that
only matters once the deployment is real: key custody, deployment ordering
under a genuine cross-contract dependency, threshold selection, and the
audit gate. This runbook covers that gap.

**This runbook does not apply to upgrading an already-deployed contract.**
For that, see [`docs/upgrade-guide.md`](upgrade-guide.md).

## 0. Pre-mainnet gate: the external audit

Before any of the steps below are executed against a real, funded mainnet
deployment, the audit requirement documented in
[`SECURITY.md`](../SECURITY.md) and tracked in #117 must be complete and
signed off. Concretely, that means:

- [ ] An external audit covering all four contracts (`compliance`,
  `invoice`, `treasury`, `settlement-workflow`) has been completed by the
  auditor(s) engaged for #117.
- [ ] All findings rated Critical or High have been remediated, and the
  remediation has itself been re-reviewed by the auditor (not just
  self-verified).
- [ ] The audit report (or a summary sufficient for sign-off) has been
  reviewed and explicitly accepted by whoever holds deployment authority for
  this protocol.
- [ ] The exact commit hash that was audited is the same commit hash being
  built and deployed in Section 2 below. If any change has landed on `main`
  since the audited commit — including a "trivial" one — treat the audit as
  not covering the delta until it is re-reviewed.

Testnet deployments (for staging/rehearsal purposes) do not require this
gate. Do not use a passed testnet deployment as a substitute for it —
testnet has no real funds at risk and does not exercise the same
threat model.

## 1. Multisig admin key management

Every contract in this protocol (`compliance`, `invoice`, `treasury`,
`settlement-workflow`) is initialized with a single admin `Address`, and
treasury additionally has a weighted multisig signer set gating
`execute_settlement` (see `contracts/treasury/src/signers.rs` and the
`multisig` crate). `scripts/init-contracts.sh` uses one throwaway
`stellar keys generate` identity called `admin` for all four contracts —
this is correct for local development and **must not** be replicated as-is
for mainnet.

### Key generation and custody

- Each signer's key must be generated on hardware the signer personally
  controls (a hardware wallet or an offline, air-gapped machine), never via
  `stellar keys generate` on a shared or cloud development host — that
  command writes the secret key to local CLI config in plaintext.
- Signer identity and role must be documented outside of this repo (in
  whatever access-controlled system the org uses for credential ownership
  records) before that signer's public key is registered on any contract.
  This runbook does not prescribe a specific custody tool; it requires that
  one exists and is used.
- No single person should hold more than one of the signer keys used to meet
  treasury's multisig threshold (Section 3). Concentrating keys defeats the
  purpose of a weighted quorum.

### Pre-launch key rotation

If a signer's key needs to be rotated before the protocol has launched (i.e.
before mainnet initialization has happened at all), this is simple: generate
a new key under the same custody process above and use the new public
address in the `initialize` / `set_signer` calls in Section 4 instead of the
compromised or lost one. No on-chain rotation flow is needed because nothing
has been initialized yet.

If rotation is needed **after** mainnet initialization, use
`Treasury::propose_signer_rotation` / the rotation-approval flow in
`contracts/treasury/src/signers.rs`, which enforces a one-hour
per-proposer cooldown (`COOLDOWN_SECS`) specifically to prevent rotation
spam from a compromised or malicious proposer. See
`docs/economic-parameters.md` for that constant and its rationale.

## 2. Deployment order

Deployment order is not arbitrary. `contracts/settlement-workflow/src/lib.rs`'s
`initialize(compliance_id, treasury_id)` **requires both the compliance and
treasury contract addresses to already exist** — settlement-workflow pins
them into its own instance storage as trusted call targets and refuses to
accept them per-call afterward (see the `#364` note in that file). This
means the deployment order is fixed:

```
1. Compliance     (no dependencies)
2. Treasury       (no dependencies)
3. Invoice        (no dependencies — independent of the settlement path)
4. Settlement-Workflow  (requires: Compliance ID, Treasury ID from steps 1–2)
```

Invoice has no dependency on the other three and can be deployed at any
point relative to them, but is listed after compliance/treasury above to
keep the settlement-critical path grouped together.

This mirrors `scripts/init-contracts.sh`'s existing order (compliance, then
invoice, then treasury) with settlement-workflow added as the final step,
since the script predates that contract's dependency requirement.

## 3. Threshold selection rationale

`Treasury::initialize(admin, threshold)` and `Treasury::update_threshold`
(`contracts/treasury/src/lib.rs`) enforce `threshold > 0` and
`threshold <= total_weight` of all registered signers, but do not themselves
pick a value — that is an operational decision, not a code default. Pick
the initial threshold using the weighted-quorum economic model described in
the multisig quorum-model documentation issue in this batch: signer weights
should reflect each signer's real-world trust/stake in the protocol, and the
threshold should be set high enough that no minority coalition of
compromised or colluding signers below the intended trust bar can reach it,
while remaining low enough that routine operations aren't blocked by
ordinary signer unavailability (vacation, lost hardware key, etc.).

Do not default to `threshold = 1` for a mainnet deployment —
`scripts/init-contracts.sh` uses `--threshold 1` only because local
development has exactly one throwaway admin identity and no real funds at
stake.

## 4. Deployment and post-deploy configuration steps

Assumes CLI identities have already been created for each signer per
Section 1, and the audit gate in Section 0 is satisfied.

```sh
NETWORK="mainnet"   # or the appropriate testnet alias

# 1. Compliance
COMPLIANCE_ID=$(stellar contract deploy --wasm .../compliance.wasm --source admin --network "$NETWORK")
stellar contract invoke --id "$COMPLIANCE_ID" --source admin --network "$NETWORK" \
  -- initialize --admin "$ADMIN_ADDRESS"

# 2. Treasury
TREASURY_ID=$(stellar contract deploy --wasm .../treasury.wasm --source admin --network "$NETWORK")
stellar contract invoke --id "$TREASURY_ID" --source admin --network "$NETWORK" \
  -- initialize --admin "$ADMIN_ADDRESS" --threshold "$THRESHOLD"

# 2a. Register each multisig signer with its weight (repeat per signer)
stellar contract invoke --id "$TREASURY_ID" --source admin --network "$NETWORK" \
  -- set_signer --admin "$ADMIN_ADDRESS" --signer "$SIGNER_ADDRESS" --weight "$SIGNER_WEIGHT"

# 3. Invoice
INVOICE_ID=$(stellar contract deploy --wasm .../invoice.wasm --source admin --network "$NETWORK")
stellar contract invoke --id "$INVOICE_ID" --source admin --network "$NETWORK" \
  -- initialize --admin "$ADMIN_ADDRESS"

# 4. Settlement-Workflow — requires COMPLIANCE_ID and TREASURY_ID from steps 1–2
WORKFLOW_ID=$(stellar contract deploy --wasm .../settlement_workflow.wasm --source admin --network "$NETWORK")
stellar contract invoke --id "$WORKFLOW_ID" --source admin --network "$NETWORK" \
  -- initialize --compliance_id "$COMPLIANCE_ID" --treasury_id "$TREASURY_ID"

# 4a. Register settlement-workflow's own contract address as a treasury signer.
# execute_with_compliance calls Treasury::execute_settlement using
# settlement-workflow's own contract address as the authorizing signer
# (contracts/settlement-workflow/src/lib.rs), so that address must carry
# enough weight in treasury's signer set to meet the threshold from step 2,
# same as any human signer would.
stellar contract invoke --id "$TREASURY_ID" --source admin --network "$NETWORK" \
  -- set_signer --admin "$ADMIN_ADDRESS" --signer "$WORKFLOW_ID" --weight "$WORKFLOW_SIGNER_WEIGHT"
```

### Post-deploy verification

- [ ] Confirm `Treasury::initialize`'s recorded threshold and signer weights
  match what was intended (Section 3) — re-read them back via the
  contract's read entrypoints rather than trusting the invocation succeeded
  silently.
- [ ] Confirm `SettlementWorkflow::initialize` was called exactly once and
  reverts with `AlreadyInitialized` on a second attempt — this is a one-time,
  irreversible pinning of the compliance/treasury addresses it trusts.
- [ ] Confirm `execute_with_compliance` succeeds end-to-end on a small,
  reversible test transaction before routing real settlement volume through
  it.

## 5. See also

- [`docs/upgrade-guide.md`](upgrade-guide.md) — upgrading a contract already
  deployed via this runbook.
- [`docs/economic-parameters.md`](economic-parameters.md) — rationale for
  cooldowns, batch caps, and TTLs referenced above.
- [`SECURITY.md`](../SECURITY.md) — audit scope and vulnerability reporting.
