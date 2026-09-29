# Settlement Workflow Contract

The Settlement Workflow contract is the enforcement point for the compliance gate
in the COMEBACKHERE payment lifecycle. Treasury does not consult the Compliance
contract itself, so this contract is what makes "a settlement only pays out to a
compliant merchant" an on-chain guarantee rather than an off-chain convention.

Concretely, it is a thin, stateless orchestrator around one cross-contract call
chain:

```
caller ──▶ SettlementWorkflow::execute_with_compliance
              │
              ├─1─▶ Compliance::is_allowed(merchant)          (compliance gate)
              │        └─ false ⇒ return Err(ComplianceCheckFailed), Treasury untouched
              │
              └─2─▶ Treasury::execute_settlement(              (payout)
                        settlement_id,
                        token_contract,
                        signer = this contract's own address
                      )
```

The workflow holds no funds and stores only two things (see [Storage](#storage)):
the pinned compliance instance and the pinned treasury instance.

## How it links the treasury and compliance contracts

- **Compliance → Treasury link (read gate).** `execute_with_compliance*` calls
  `Compliance::is_allowed` through the `ComplianceClient` in
  `crates/compliance-client`. If the merchant is blocked, not allowed, or holds
  an expired time-bound allow, the call short-circuits with
  `Err(TreasuryError::ComplianceCheckFailed)` and the treasury is never called —
  no partial state, no wasted treasury invocation.
- **Workflow → Treasury link (write path).** After the gate passes, the workflow
  calls `Treasury::execute_settlement` with **its own address** as the
  authorizing `signer`. This is the crux of the wiring: the workflow contract
  must be registered as a Treasury signer via `Treasury::set_signer` before any
  settlement can flow through it. Without that registration the nested call fails
  with `TreasuryError::WorkflowNotRegisteredSigner`, not a generic
  `UnauthorizedSigner` — the dedicated code exists so a first-time deployer gets
  pointed straight at the `set_signer` call they are missing.
- **Cross-contract client surface.** Treasury is reached via a local
  `#[contractclient]` trait (`TreasuryInterface`) rather than a direct dependency
  on the `comebackhere-treasury` crate. Depending on the implementation crate
  would statically link treasury's own wasm exports (`pause`, `unpause`, …)
  alongside this contract's and collide at link time. `compliance` and `treasury`
  are therefore **dev-dependencies only** in `Cargo.toml` — needed by the tests
  to stand up real instances, never linked into the shipped wasm.

## Entrypoints

| Function | Auth Required | Parameters | Returns | Errors |
|----------|---------------|------------|---------|--------|
| `initialize` | None (deployer) | `compliance_id: Address, treasury_id: Address` | `()` | Traps with `AlreadyInitialized` on a second call |
| `execute_with_compliance` | None at this contract's boundary — see [Who can call what](#who-can-call-what) | `settlement_id: u64, token_contract: Address, merchant: Address` | `Result<(), TreasuryError>` | `ComplianceCheckFailed` |
| `execute_with_compliance_batch` | None at this contract's boundary — see [Who can call what](#who-can-call-what) | `settlement_ids: Vec<u64>, token_contract: Address, merchant: Address` | `Result<Vec<u64>, TreasuryError>` | `ComplianceCheckFailed` |

### `initialize`

Pins the compliance and treasury contract instances this workflow trusts. Must be
called exactly once, before any `execute_with_compliance*` call; a second call
traps with `TreasuryError::AlreadyInitialized` (there is deliberately no
"re-point the workflow" path).

Pinning at initialization rather than per-call is what stops a caller from
redirecting the compliance gate at an arbitrary — potentially attacker-owned —
compliance or treasury instance on any given call.

Emits: `workflow_initialized` → `(compliance_id, treasury_id)`.

### `execute_with_compliance`

Runs the compliance gate for `merchant` once, then executes a single settlement
through the pinned treasury. Returns `Ok(())` only when the merchant is allowed
*and* the treasury accepted the execution.

Emits: `settlement_workflow_executed` → `(merchant, token_contract)`, so an
indexer can distinguish a compliance-gated payout from a settlement executed
directly against treasury.

### `execute_with_compliance_batch`

Batch variant of the above. The shared compliance gate for `merchant` is
evaluated **once** for the whole batch, then each settlement ID is executed in
order.

Settlement IDs that don't exist, are already executed, or otherwise fail
treasury execution are **silently skipped** rather than aborting the batch —
matching treasury's own batch precedent, so one bad ID cannot grief the rest of
an operator's batch. The returned `Vec<u64>` therefore contains only the IDs
that actually executed, in input order.

The one failure that *does* reject the entire batch is a failed compliance gate
(`Err(ComplianceCheckFailed)`), because there is no partial credit for a
non-compliant merchant.

Emits: `settlement_workflow_executed` → `(merchant, token_contract)` once per
settlement actually executed.

## Who can call what

There is no `require_auth` in this contract, and that is deliberate: the
authorization lives one hop downstream, in Treasury.

- **`initialize`** — callable by anyone, but only the first call wins. Practically
  this is a deployer step: whoever deploys the contract should call it in the same
  transaction, before anything else. If it is left uninitialized, every
  `execute_with_compliance*` call panics on the `unwrap()` of the missing
  `ComplianceId`, and if it is initialized by the wrong party, the workflow is
  permanently pinned to that party's contract IDs with no remedy.
- **`execute_with_compliance` / `execute_with_compliance_batch`** — callable by
  anyone at the workflow's own boundary, but the nested
  `Treasury::execute_settlement` is authorized as the **workflow contract's**
  address. So the effective permission is "whatever the workflow contract will
  do on your behalf": a settlement still has to clear the treasury's threshold,
  the token has to be on the settlement allowlist, and the merchant has to pass
  the compliance gate. A caller cannot bypass any of those by calling the
  workflow directly.

Because the workflow is the signer, registration is a one-time treasury admin
action:

```sh
stellar contract invoke \
  --id $TREASURY_CONTRACT \
  --source $ADMIN \
  --network $NETWORK \
  -- set_signer \
  --admin $ADMIN \
  --signer $SETTLEMENT_WORKFLOW_CONTRACT \
  --weight 1
```

## Storage

| Key | Type | Scope | Meaning |
|---|---|---|---|
| `ComplianceId` | `Address` | Instance | Pinned compliance contract instance |
| `TreasuryId` | `Address` | Instance | Pinned treasury contract instance |
| `ExecutedSettlements` | `Vec<u64>` | Persistent | Reserved for the ordered list of settlement IDs executed through this gated path |

`ExecutedSettlements` is declared but not yet read or written by any entrypoint —
it is reserved for the settlement-history export tracked in #373. The variant
exists so the `DataKey` enum stays append-only (reordering would break stored
data keyed by ordinal position).

## CLI usage examples

Replace `$SETTLEMENT_WORKFLOW_CONTRACT`, `$COMPLIANCE_CONTRACT`,
`$TREASURY_CONTRACT`, `$MERCHANT`, `$TOKEN`, `$SETTLEMENT_ID`, and `$NETWORK`
with your deployed values.

### initialize

```sh
stellar contract invoke \
  --id $SETTLEMENT_WORKFLOW_CONTRACT \
  --source $DEPLOYER \
  --network $NETWORK \
  -- initialize \
  --compliance_id $COMPLIANCE_CONTRACT \
  --treasury_id $TREASURY_CONTRACT
```

### execute_with_compliance

```sh
stellar contract invoke \
  --id $SETTLEMENT_WORKFLOW_CONTRACT \
  --source $CALLER \
  --network $NETWORK \
  -- execute_with_compliance \
  --settlement_id $SETTLEMENT_ID \
  --token_contract $TOKEN \
  --merchant $MERCHANT
```

### execute_with_compliance_batch

```sh
stellar contract invoke \
  --id $SETTLEMENT_WORKFLOW_CONTRACT \
  --source $CALLER \
  --network $NETWORK \
  -- execute_with_compliance_batch \
  --settlement_ids '[1,2,3]' \
  --token_contract $TOKEN \
  --merchant $MERCHANT
```

## Running the tests

The integration tests stand up real `ComplianceContract` and
`TreasuryContract` instances alongside the workflow, so they exercise the actual
two-hop cross-contract call chain rather than mocks:

```sh
cargo test --package comebackhere-settlement-workflow
```

To watch the instruction-budget and event assertions, run with output:

```sh
cargo test --package comebackhere-settlement-workflow -- --nocapture
```

The suite covers both compliance-gate branches (pass and fail), the pinned-
instance behaviour of `initialize`, batch execution with skipped invalid IDs,
whole-batch rejection on a failed gate, idempotency of a retried
`execute_with_compliance` (no double payout), and a generous CPU-instruction
ceiling over the composed call chain so a large future regression shows up as a
test failure rather than a surprise on mainnet.

## Running the fuzz target

`contracts/settlement-workflow/fuzz` is a **standalone workspace** (it has its
own `[workspace]` table), deliberately kept out of the root workspace because
`cargo-fuzz` requires nightly and the libfuzzer runtime. To run it locally:

```sh
cargo install cargo-fuzz          # once
cd contracts/settlement-workflow/fuzz
cargo +nightly fuzz run execute_with_compliance
```

or, from the repo root:

```sh
just fuzz-settlement-workflow
```

The `execute_with_compliance` target throws adversarial address and `u64`
combinations at the workflow to catch panics that logic-only assertions would
miss — overflow, `unwrap()`-on-`None`, and combinations that would let a caller
treat the wrong contract instance as authoritative. It is also run on a weekly
schedule by `.github/workflows/fuzz.yml` with a 120-second bound per target.

> **Known issue:** the harness in `fuzz_targets/execute_with_compliance.rs`
> still predates the #364 change that moved `compliance_id` / `treasury_id` out
> of the per-call signature, and its `Cargo.toml` path dependencies resolve to
> `contracts/contracts/...`. `cargo check` in that directory therefore fails
> until both are updated. The root-workspace `cargo test --workspace` and CI
> pre-commit jobs are unaffected, since the fuzz crate is not a workspace member.
