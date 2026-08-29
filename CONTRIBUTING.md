# Contributing

## Getting started

An issue must be assigned to you before you begin work. Comment on the issue to request assignment.

## Branch naming

Branch names must follow the pattern `<type>/<short-description>` using lowercase kebab-case:

| Prefix | When to use |
|--------|-------------|
| `feat/` | New feature or behaviour |
| `fix/` | Bug fix |
| `test/` | Adding or improving tests |
| `chore/` | Build, tooling, dependency updates |
| `docs/` | Documentation only |

Examples:
```
feat/invoice-expiry
fix/treasury-overflow
docs/contributing-guidelines
```

## Commit messages

Follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).

```
<type>(<optional scope>): <short summary>

[optional body]

[optional footer: Closes #<issue_id>]
```

- **type** — one of `feat`, `fix`, `test`, `chore`, `docs`, `refactor`, `perf`, `ci`
- **scope** — contract name or area, e.g. `invoice`, `treasury`, `compliance`
- **summary** — imperative mood, no period, ≤72 characters
- **footer** — include `Closes #<issue_id>` when the commit resolves an issue

Examples:
```
feat(invoice): add expiry timestamp to invoice state
fix(treasury): prevent integer overflow in settlement math
docs: expand CONTRIBUTING.md with branch, commit, and PR guidelines
```

## Pull requests

### Before opening a PR

- [ ] Pre-commit hooks pass locally (`pre-commit run --all-files`)
- [ ] All tests pass (`cargo test`)
- [ ] Clippy is clean (`cargo clippy -- -D warnings`)
- [ ] ABI snapshots regenerated if contract interfaces changed

### PR description template

```
## Summary

<!-- What does this PR do and why? -->

## Changes

<!-- Bullet list of notable changes -->

## Testing

<!-- How was this tested? Include test names or commands run -->

## ABI impact

<!-- Does this change a contract interface? If yes, confirm snapshots are updated -->

Closes #<issue_id>
```

The `Closes #<issue_id>` line is required. PRs without a linked issue will not be merged.

## Code review

- Reviewers aim to respond within **2 business days**.
- Address all comments before requesting a re-review. Resolve threads only after the reviewer approves or explicitly says the comment is addressed.
- Keep review feedback focused on correctness, safety, and consistency with existing patterns. Style issues are caught by pre-commit hooks.
- A PR requires **at least one approving review** before merge.
- Squash-merge into `main`; the PR title becomes the merge commit message (must be a valid Conventional Commit).

## Local hooks

Install [pre-commit](https://pre-commit.com/) and enable the repository hooks:

```sh
pip install pre-commit
pre-commit install
```

Hooks run on each commit and enforce:

- `cargo fmt --all -- --check`
- `cargo clippy -- -D warnings`

Run all hooks manually:

```sh
pre-commit run --all-files
```

## ABI snapshots

After changing contract interfaces, regenerate and verify ABI metadata from the `COMEBACKHERE/` repo:

```sh
# In COMEBACKHERE/
make update-abi-snapshots
# or
just snapshot
```

---

## Deprecating an entrypoint

(#459) Removing or replacing a public entrypoint outright — rather than just extending it — can silently break off-chain integrators or other contracts that already call it in production. This project already has direct experience with what happens when an interface drifts without anyone noticing (the invoice contract's function/event set fell out of sync with `abis/invoice.json` before `abi-drift-check.yml` existed to catch it). Follow this process instead of removing an entrypoint directly.

### Policy

1. **Mark it deprecated, don't remove it.** Add a `#[deprecated(note = "...")]` (or an equivalent doc comment if the attribute can't be used on a `#[contractimpl]` fn in a given case) pointing callers at the replacement, and add a one-line note to the contract's README entrypoints table. The entrypoint keeps working exactly as before — no panic, no behavior change — for the full notice period.
2. **Emit a deprecation-signal event on every call**, e.g. `("<fn>_deprecated",)`, so off-chain indexers can detect live usage and reach out to or identify remaining callers instead of discovering the removal only when it happens. Document the event in `docs/event-schema.md` (see below) like any other event.
3. **Minimum notice period: one minor version.** An entrypoint marked deprecated in version `X.Y.0` may only be removed at version `X.(Y+1).0` or later — never a patch release, and never in the same release that introduces the deprecation. This gives integrators at least one full release cycle of advance notice, matching a Conventional Commits-style versioning scheme where removal of a public interface is itself a `feat`/breaking-change entry.
4. **Removal happens at that version boundary, not before**, and is itself a normal PR following this same CONTRIBUTING.md process (new branch, `feat`/breaking-change commit calling out the removal explicitly, CHANGELOG entry).

### Interaction with ABI drift checks

`abi-drift-check.yml` diffs the function and event set extracted from source against `abis/*.json` and fails the build on any mismatch — a removed function is exactly the kind of change it's designed to catch. That means:

- Marking an entrypoint deprecated (step 1 above) does not touch the function set, so it does not trip the drift check — the function is still present.
- Actually removing the entrypoint at the version boundary (step 4) *is* a real ABI change and must be paired with `make update-abi-snapshots` / `just snapshot` in the same PR (see **ABI snapshots** above) — a PR that removes a function without a matching snapshot update will correctly fail CI.
- `abi-drift-check.yml` today only watches `contracts/invoice/src/**` and `abis/invoice.json`; as ABI snapshots expand to cover other contracts, this deprecation policy applies to their entrypoints identically, and the workflow's `paths` filter should be extended alongside the new snapshot.

---

## Adding a new contract to the workspace

Follow these steps in order. Each step references the file(s) to edit.

### 1. Create the crate

```sh
mkdir -p contracts/<name>/src
```

Add a minimal `contracts/<name>/Cargo.toml`:

```toml
[package]
name = "comebackhere-<name>"
version = "0.1.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[lib]
crate-type = ["cdylib"]

[dependencies]
soroban-sdk = { workspace = true }
```

### 2. Register in the workspace (`Cargo.toml`)

Add `"contracts/<name>"` to the `members` array in the root `Cargo.toml`:

```toml
[workspace]
members = [
    ...
    "contracts/<name>",
]
```

### 3. Claim an error-code range (`deny.toml` + source)

Error-code ranges are documented in the **Error-Code Ranges per Contract** table in `ARCHITECTURE.md` (see also issue #73). The current allocations are:

| Contract | Range |
|---|---|
| `invoice` | 1–21 |
| `treasury` | 1–17 |
| `compliance` | 1–4 |

Because each contract has its own `#[contracterror]` enum the numeric ranges are per-contract, not global. Pick a starting range that leaves room for growth and document it in `ARCHITECTURE.md` under **Error-Code Ranges per Contract**. Append new variants only — never renumber existing ones.

`deny.toml` itself needs no change for a new contract; it governs dependency licenses and advisories, not error codes. Verify `cargo deny check` still passes after adding any new dependencies.

### 4. Update CI workflows

**`build.yml`** — add an explicit build step for the new contract:

```yaml
- name: Build <name> contract (WASM)
  run: cargo build --target wasm32-unknown-unknown --release -p comebackhere-<name>
```

**`contract-size.yml`** — no change needed; the loop over `contracts/*/` picks up new contracts automatically.

**`test.yml`** — no change needed; `cargo test --workspace` covers all workspace members.

### 5. Update documentation

- **`README.md`** — add a row to the contract table:

  ```markdown
  | `<name>` | One-line description of what the contract does |
  ```

- **`ARCHITECTURE.md`** — add:
  - A row in the **Contracts** table.
  - A **DataKey Storage Reference** section for the new contract.
  - A row in the **Error-Code Ranges per Contract** table with the chosen range.
  - Any new cross-contract call edges in the **Cross-Contract Call Map**.

- **`contracts/<name>/README.md`** — create a README following the same structure as the existing contract READMEs (entrypoints table, auth, params, returns, errors, CLI examples).

### 6. Pre-merge checklist

- [ ] `cargo fmt --all` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo deny check` passes
- [ ] ABI snapshots regenerated if the new contract exposes a public interface
- [ ] All documentation steps above completed
