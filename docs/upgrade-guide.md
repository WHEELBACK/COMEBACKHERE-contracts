# Contract Upgrade Guide

This document is the operational, step-by-step guide for upgrading a contract
that is **already deployed** at a live Soroban contract address, as opposed to
deploying it fresh. If you have never done a Soroban contract upgrade before,
read this whole document before touching a real deployment — the mechanics
and the failure modes are meaningfully different from a fresh `deploy`.

`scripts/init-contracts.sh` is the reference for a **fresh** deploy: it calls
`stellar contract deploy` once per contract, gets back a brand-new contract
ID, and calls `initialize` on it. An upgrade is a different operation: the
contract ID stays the same, the WASM executable behind it changes, and
instance/persistent storage that was already written under the old code is
now read by the new code. Get that storage compatibility wrong and you can
brick a live deployment — there is no `deploy` do-over once real funds are
held by the contract.

This guide is scoped to the mechanics of the upgrade itself (which CLI
commands, in which order, with which pre-flight checks). For what makes a
storage layout *change* safe or unsafe in the first place, see
[`ARCHITECTURE.md`'s "Storage Migration Strategy"](../ARCHITECTURE.md#storage-migration-strategy)
(originating from #113) — that section is the canonical reference for the
additive-only field rule and the `migrate(admin: Address)` entrypoint
pattern. This guide does not duplicate that content; it tells you when and
how to invoke the CLI commands that put an already-migration-safe WASM binary
onto a live contract address.

## 1. How Soroban storage survives (or doesn't survive) an upgrade

A Soroban contract address is bound to a **WASM hash**, not the other way
around. `stellar contract deploy` creates a new contract instance with a
fresh ID and installs a WASM hash behind it. An **upgrade**, by contrast,
keeps the existing contract ID and instance storage in place, and only swaps
out which WASM hash that ID points to.

Concretely, for this repo's contracts:

- **Instance storage** (`env.storage().instance()`) — used for `Admin`,
  `Paused`, `Threshold`, `SignerList`, compliance's `AddressIndex`, etc. —
  lives at the ledger entry for the contract *instance*, not the WASM code.
  It is untouched by an upgrade. The new WASM's `#[contracttype]` definitions
  simply read the bytes that are already there, using whatever XDR
  deserialization those types imply.
- **Persistent storage** (`env.storage().persistent()`) — used for
  compliance's per-address `Allowed(Address)` / `Blocked(Address)` entries,
  treasury's `Settlement` records, invoice's `Invoice` records — is likewise
  keyed independently of the WASM hash and survives an upgrade unchanged.
- **What does *not* automatically survive** is compatibility between the old
  struct/enum layout and the new one. If the new WASM's `#[contracttype]`
  removes a field, reorders one, or changes a variant's discriminant that
  existing stored bytes were written against, deserializing that old data
  with the new type will fail at runtime (not at deploy time — the upgrade
  transaction itself will succeed either way). This is exactly the class of
  problem the additive-only rule in `ARCHITECTURE.md` exists to prevent.

In short: the upgrade transaction only ever changes code. Whether that code
change is *safe* given what's already in storage is entirely on you to
verify beforehand — Soroban will not warn you.

## 2. The actual CLI commands

This repo pins Stellar CLI `22.8.2` (see `README.md`'s toolchain table and
`.github/workflows/build.yml`'s `STELLAR_CLI_VERSION`). All commands below
are written against that version. Confirm your local CLI matches before
running any of this against a real deployment:

```sh
stellar --version
# stellar 22.8.2 ...
```

### Step 1 — Build and locate the new WASM

Same build invocation `scripts/init-contracts.sh` uses, scoped to the
package you're upgrading (do not use `--workspace`, for the same reason the
script avoids it — the `comebackhere-tests` crate's `testutils` feature is
incompatible with the `wasm32-unknown-unknown` target):

```sh
cargo build --target wasm32-unknown-unknown --release -p comebackhere-treasury
```

### Step 2 — Install the new WASM (upload the code, do not touch the instance yet)

`stellar contract install` uploads a WASM blob to the network and returns its
hash, without touching any existing contract instance:

```sh
stellar contract install \
  --wasm target/wasm32-unknown-unknown/release/treasury.wasm \
  --source upgrade-admin \
  --network "$NETWORK"
```

Record the returned WASM hash. This step is fully reversible and has no
effect on the live contract — it is safe to run against mainnet at any time,
including well before the actual upgrade window, so you can decouple "build
and upload the candidate" from "flip the switch."

### Step 3 — Invoke the contract's own upgrade entrypoint

Unlike a fresh deploy, an in-place upgrade is performed by invoking the
`upgrade` function that the Soroban SDK's `#[contractimpl]` machinery exposes
on the deployed contract itself (backed by `env.deployer().update_current_contract_wasm(new_hash)`
inside the contract), not by a standalone `stellar contract update` CLI verb.
Concretely:

```sh
stellar contract invoke \
  --id "$TREASURY_ID" \
  --source upgrade-admin \
  --network "$NETWORK" \
  -- upgrade \
  --new_wasm_hash "$NEW_WASM_HASH"
```

This is the step that actually changes what code runs at `$TREASURY_ID`. The
contract ID, all instance storage, and all persistent storage are
unaffected — only the code being executed changes. If the contract does not
currently expose an `upgrade` entrypoint gated on admin auth, that entrypoint
must be added (and deployed once, the ordinary way) before an in-place
upgrade is possible at all; retrofit it well ahead of the actual upgrade you
need to perform, not during it.

### Step 4 — Verify

```sh
stellar contract invoke \
  --id "$TREASURY_ID" \
  --source anyone \
  --network "$NETWORK" \
  -- get_schema_version   # or any read-only entrypoint proving the new code path
```

Cross-check against events emitted by the upgrade itself, and confirm a
sample of pre-upgrade state (e.g. an existing `Settlement`) still reads back
correctly through the new code before declaring the upgrade complete.

## 3. Pre-upgrade checklist (run and confirm clean before touching a live deployment)

None of the following steps are optional for a real, funded deployment. Each
one exists because getting an in-place upgrade wrong is categorically worse
than a bad testnet deploy — there is no "just redeploy" recovery path once
live funds are behind the contract address.

- [ ] **ABI-drift snapshot check is clean for the contract being upgraded.**
  Once the ABI snapshot tooling referenced in the related batch of issues
  exists for all four contracts, run it against the new WASM before
  installing anything. A clean diff means the new WASM's exported function
  signatures and `#[contracttype]` shapes are compatible with what callers
  (including the other three contracts, via their generated clients) already
  expect. Do not proceed on a dirty snapshot without an explicit, reviewed
  justification for the drift.
- [ ] **`ARCHITECTURE.md`'s Storage Migration Strategy has been followed.**
  Confirm every storage-shape change in the diff is either additive-only
  (`Option<T>` new fields) or accompanied by an explicit `migrate(admin)`
  entrypoint per that section. Confirm the per-contract migration risk notes
  in that same section still match reality for the contract you're touching.
- [ ] **`cargo test --all`, `cargo clippy -- -D warnings`, and
  `cargo fmt --all -- --check` all pass** against the exact commit being
  installed.
- [ ] **The new WASM has been installed (Step 2) and its hash independently
  verified** (e.g. by a second person recomputing the hash from the same
  source commit) before the `upgrade` invocation is signed.
- [ ] **A rollback plan exists.** Because instance/persistent storage is
  untouched by an upgrade, rolling back is itself just another `upgrade`
  invocation pointing at the previous WASM hash — but only if that previous
  hash is still installed and its storage shape is still compatible with
  whatever writes happened under the new code in between. Do not assume
  rollback is free; verify it before you need it.
- [ ] **Multisig/admin signing follows the process in
  `docs/deployment-runbook.md`.** The same key-custody and quorum concerns
  that apply to initial mainnet deployment apply to every subsequent
  upgrade — an upgrade transaction is at least as sensitive as an initial
  deploy, since it can replace the entire logic of an already-funded
  contract.

## 4. What this guide deliberately does not cover

- The rules for *which* storage-shape changes are safe: see
  `ARCHITECTURE.md`'s Storage Migration Strategy (#113).
- Who is authorized to sign an upgrade transaction and how that key material
  is managed: see `docs/deployment-runbook.md`.
- Generic Soroban/Stellar CLI documentation not specific to this repo's
  contracts or pinned tool versions — refer to the official Stellar docs for
  anything not covered above.
