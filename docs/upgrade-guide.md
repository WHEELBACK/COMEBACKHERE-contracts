# Upgrade Guide: Redeploying `settlement-workflow`

> **Glossary:** For term definitions, see [`GLOSSARY.md`](GLOSSARY.md). For contract
> data storage, see [`../ARCHITECTURE.md`](../ARCHITECTURE.md).

## Why this exists

`SettlementWorkflowContract::initialize(compliance_id, treasury_id)` pins the
compliance and treasury instances a given workflow deployment trusts, and traps with
`AlreadyInitialized` if called a second time on the same instance
(`contracts/settlement-workflow/src/lib.rs`). There is no in-place way to repoint an
already-initialized instance at a different `compliance_id` or `treasury_id` — any
upgrade, bug fix, or configuration change that requires a new `compliance_id` /
`treasury_id` pinning means **deploying an entirely new `settlement-workflow`
instance**, not upgrading the existing one.

Because `Treasury::execute_settlement` only accepts calls from addresses registered
as signers (`TreasuryOnlyClient` in `settlement-workflow`, gated by
`require_authorized_signer` in treasury), a new workflow instance is inert — it can
pass its own compliance check but every `execute_settlement` call will fail with
`UnauthorizedSigner` — until it is explicitly registered as a treasury signer via
`Treasury::set_signer`. This guide documents the cutover sequence so that migrating
never leaves a window with **no working entry point** for compliance-gated
settlements.

> **Note:** `settlement-workflow` has no `pause` entrypoint today. "Disabling" the old
> instance during cutover means removing its treasury signer weight
> (`Treasury::remove_signer` / `Treasury::set_signer(..., weight=0)`) and no longer
> directing off-chain traffic to it — not toggling a paused flag. If a pause mechanism
> is added to `settlement-workflow` in the future, prefer pausing over signer removal
> for step 4 below, since it is instantly reversible without a treasury call.

## Cutover sequence

Given an **old** instance `OLD_WORKFLOW` (already a registered treasury signer) and a
**new** instance `NEW_WORKFLOW` being deployed to replace it:

1. **Deploy the new instance.**
   ```sh
   stellar contract deploy \
     --wasm target/wasm32v1-none/release/comebackhere_settlement_workflow.wasm \
     --source $ADMIN --network $NETWORK
   # → NEW_WORKFLOW contract ID
   ```

2. **Initialize the new instance**, pinning it at the *same* compliance and treasury
   instances the old one used (or new ones, if this migration is itself part of a
   compliance/treasury redeploy):
   ```sh
   stellar contract invoke --id $NEW_WORKFLOW --source $ADMIN --network $NETWORK \
     -- initialize \
     --compliance_id $COMPLIANCE_CONTRACT \
     --treasury_id $TREASURY_CONTRACT
   ```
   `initialize` requires no auth beyond the deploy transaction itself and emits
   `workflow_initialized`; verify the emitted event before proceeding.

3. **Register the new instance as an authorized treasury signer** *before* removing
   the old one, so there is no gap where zero workflow instances can call
   `execute_settlement`:
   ```sh
   stellar contract invoke --id $TREASURY_CONTRACT --source $ADMIN --network $NETWORK \
     -- set_signer \
     --admin $ADMIN \
     --signer $NEW_WORKFLOW \
     --weight $SIGNER_WEIGHT
   ```
   `$SIGNER_WEIGHT` should match whatever weight `OLD_WORKFLOW` currently holds
   (check via `Treasury::get_signer_weight --signer $OLD_WORKFLOW`), so the new
   instance can meet the same approval threshold the old one could.

4. **Verify the new instance end-to-end before cutting over traffic.** Confirm
   `Compliance::is_allowed` and a dry-run `execute_with_compliance` (or
   `execute_with_compliance_batch`) against `NEW_WORKFLOW` succeed for a known-good
   test address/settlement on the target network.

5. **Redirect off-chain callers** (indexers, the service that submits
   `execute_with_compliance*` transactions) from `OLD_WORKFLOW` to `NEW_WORKFLOW`.

6. **Deregister the old instance as a treasury signer**, only after step 5 is
   confirmed complete — this is what actually revokes `OLD_WORKFLOW`'s ability to
   execute settlements:
   ```sh
   stellar contract invoke --id $TREASURY_CONTRACT --source $ADMIN --network $NETWORK \
     -- remove_signer \
     --admin $ADMIN \
     --signer $OLD_WORKFLOW
   ```
   `remove_signer` does not retroactively invalidate settlements already executed
   through `OLD_WORKFLOW`, so this step is safe to run at any point once traffic has
   moved.

## Ordering rationale

Steps 1–3 (deploy → configure → register as signer) happen entirely **before** step 6
(deregister the old signer). This ordering means both workflow instances are valid,
authorized entry points during the overlap window between steps 3 and 6 — the
opposite ordering (deregistering `OLD_WORKFLOW` first) would create exactly the gap
this guide exists to avoid: a period where compliance-gated settlements have no
working entry point at all, because the old instance has been cut off and the new one
isn't registered yet.

## Rollback

If `NEW_WORKFLOW` fails verification (step 4) after being registered as a signer
(step 3) but before traffic has moved (step 5), simply `remove_signer` on
`NEW_WORKFLOW` — `OLD_WORKFLOW` was never deregistered, so it continues serving
traffic unaffected. There is no rollback needed for `initialize` itself: an
`initialize`d instance that is never registered as a treasury signer, or never
receives traffic, has no side effects on the old instance or on treasury/compliance
state.
