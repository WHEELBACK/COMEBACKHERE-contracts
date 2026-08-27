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

## Changing `crates/multisig`

`crates/multisig` is a shared dependency: `contracts/treasury` re-exports its
contract types (`Settlement`, `Dispute`, `SettlementStatus`, `DisputeStatus`,
`RotationStatus`, `SettlementHoldReason`, `SignerRotationProposal`,
`TreasuryError`, ...) directly through treasury's own public ABI. Because
that re-export is a normal Rust `pub use`, a semver-compatible change to one
of those types (a renamed field, an added or reordered enum variant, a
changed discriminant) compiles cleanly but can silently change treasury's
on-chain ABI with no compiler error to catch it — see issue #68.

The enforcement mechanism for this lives entirely in
[`contracts/treasury/tests/multisig_version_lock_test.rs`](contracts/treasury/tests/multisig_version_lock_test.rs).
That file exhaustively matches/destructures every ABI-relevant multisig type
with no wildcard arm and no `..` struct spread, so it fails to *compile* the
moment one of those shapes changes. If you're touching `crates/multisig` for
any reason, expect to go through this process:

1. Make your change to `crates/multisig`.
2. Try to compile `contracts/treasury`'s tests. `multisig_version_lock_test.rs`
   will fail to compile if your change touched any exhaustively-matched
   field or variant.
3. Update every match arm / struct destructure in that file until it compiles
   again against the new shape. This is a deliberate manual step — it forces
   a human to look at the ABI impact of the change, not just accept whatever
   the compiler allows.
4. Only once it compiles, regenerate the treasury ABI snapshot (see "ABI
   snapshots" above) and bump both `crates/multisig/Cargo.toml`'s
   `version` and the test file's `EXPECTED_MULTISIG_VERSION` constant
   together, in the same commit.

Read the doc comment at the top of `multisig_version_lock_test.rs` for the
full rationale; this section only summarizes it so the process is
discoverable before you break the compile gate, not after.

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
