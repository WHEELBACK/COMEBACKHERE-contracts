# PR review checklist

> **Status:** Living checklist · Reviewer aid, not a substitute for CI
> **Scope:** Sits between [`SECURITY.md`](../SECURITY.md) (how to report a
> vulnerability after the fact) and [`docs/audit-scope.md`](./audit-scope.md)
> (what an external auditor covers once commissioned). This document is the
> lightweight, repo-specific checklist a reviewer runs on every incoming PR
> *before* either of those apply.

Every item below is distilled from a real incident, closed issue, or CI check
already living in this repo's history — not generic security-checklist
boilerplate. If a check here duplicates something CI already enforces, that's
intentional: CI catches the mechanical violation, this checklist catches the
judgment call a human has to make before the violation ever reaches CI (or in
cases where CI's coverage has known gaps).

Use this alongside — not instead of — the "Before opening a PR" checklist in
[`CONTRIBUTING.md`](../CONTRIBUTING.md). Nothing here contradicts that file;
where the two overlap (fmt/clippy/tests), CONTRIBUTING.md is authoritative on
mechanics and this file is authoritative on what to look for.

## 1. Panics and contract size

- [ ] Does this PR add a new `panic!()` (or `.unwrap()` / `.expect()` on a
      `Result`/`Option` that can plausibly be `Err`/`None` in production) inside
      contract entrypoint code, where `panic_with_error!(env, SomeError)` should
      be used instead?
  - **Why this matters here:** the `Contract Size` CI job
    (`.github/workflows/contract-size.yml`) enforces a hard 64 KB WASM budget
    per contract. Rust's default panic machinery (formatting, location
    tracking) pulls in `core::fmt` machinery that bloats WASM size far more
    than `soroban_sdk::panic_with_error!`'s compact trap-with-error-code path.
    Treasury has already failed this CI check for exactly this reason (see
    `fix(ci): scope init-contracts.sh build, optimize wasm to fit size budget`
    and the related `--all-features` wasm-opt fix). Grep the diff for `panic!(`
    and `.unwrap()`/`.expect()` on entrypoint-reachable paths; every existing
    entrypoint in `treasury`, `invoice`, and `compliance` uses
    `soroban_sdk::panic_with_error!` or returns `Result<_, XError>` instead.
- [ ] If this PR touches any contract's `Cargo.toml`, `src/lib.rs` feature
      flags, or adds a new dependency, was the contract's WASM size actually
      checked against the 64 KB budget locally (or will CI catch it before
      merge)? New dependencies pulled into a `[dependencies]` (not
      `[dev-dependencies]`) entry are the most common way this creeps up.

## 2. `#[repr(u32)]` enum ordering (append-only)

- [ ] Does this PR add a new variant to any `#[contracterror]` or
      `#[contracttype]` `#[repr(u32)]` enum (e.g. `TreasuryError`,
      `InvoiceError`, `ContractError`, `ComplianceError`)?
  - If yes: is the new variant appended **after the highest existing
    discriminant**, with no renumbering, reordering, or insertion in the
    middle?
  - **Why this matters here:** issue #74 formalized this as an append-only
    policy and added `scripts/check-enum-ordering.sh` to CI to catch
    discriminant gaps or reorderings automatically. But the script parses
    explicit `= N` discriminants line-by-line and skips files under
    `/tests/` — it cannot catch every possible mistake (e.g. a variant
    inserted with the *correct next* discriminant but in the *wrong position*
    relative to related variants that share semantic grouping, or a rename
    that happens to reuse a stale discriminant on a fresh branch that hasn't
    rebased). Read the enum's surrounding comment block — every append-only
    enum in this repo documents its own invariant inline (see
    `ContractError` in `contracts/compliance/src/lib.rs` and `TreasuryError`
    in `crates/multisig/src/lib.rs`) — and confirm the PR's diff actually
    matches what that comment promises, not just what the script happens to
    pass.
  - Run `./scripts/check-enum-ordering.sh` locally rather than trusting CI
    caught it; the script's own header notes it is a best-effort static
    parse, not a compiler-enforced guarantee.

## 3. Cross-contract dependencies and duplicate WASM exports

- [ ] Does this PR add a new dependency from one contract (or a `*-client`
      crate) onto another contract crate — e.g. `settlement-workflow`
      depending on `compliance` or `treasury`?
  - If yes: is the dependency on the *contract* crate (which carries a
    `#[contractimpl]` block and therefore exports WASM symbols like `pause`,
    `unpause`, `initialize`) scoped to `[dev-dependencies]` only, with the
    real cross-contract call path going through a dedicated `*-client` crate
    (e.g. `compliance-client`) that depends only on shared types
    (`multisig`, plain `#[contracterror]`/`#[contracttype]` definitions) and
    never on the other contract's `#[contractimpl]` block?
  - **Why this matters here:** PR #358 (`fix: make CI green (link failure,
    size budget, stale pins, lint)`) fixed exactly this failure mode —
    `settlement-workflow` linking both `treasury` and `compliance` pulled in
    duplicate `pause`/`unpause` WASM exports and broke the build at link
    time. `crates/compliance-client/Cargo.toml` now documents this trap
    explicitly in a comment above its `[dev-dependencies]` block; read that
    comment before adding any new `[dependencies]` entry that points at a
    `contracts/*` crate rather than a `crates/*-client` crate.
- [ ] If this PR adds a dependency in `[dependencies]` (not
      `[dev-dependencies]`) on another contract crate, does that dependency's
      justification hold up — is the type being depended on a plain error/data
      enum with no `#[contractimpl]` body (safe, see the `multisig` dependency
      pattern), or does it risk re-linking that contract's own exports?

## 4. Multisig / treasury ABI drift

- [ ] Does this PR change any type in `crates/multisig` (fields, variants,
      ordering) that `contracts/treasury` re-exports or accepts/returns
      directly (`Settlement`, `Dispute`, `SignerRotationProposal`,
      `SettlementStatus`, `DisputeStatus`, `RotationStatus`,
      `SettlementHoldReason`, `TreasuryError`)?
  - If yes: does `contracts/treasury/tests/multisig_version_lock_test.rs`
    still compile, and has `EXPECTED_MULTISIG_VERSION` in that file been
    bumped alongside `crates/multisig/Cargo.toml`'s `version` field?
  - **Why this matters here:** this file exists specifically because a
    multisig crate bump can be a semver-compatible Rust change that silently
    changes treasury's on-chain ABI without a compile error (issue #68). The
    exhaustive match in that test is the enforcement mechanism — if it still
    compiles after your change, the ABI-relevant shape is unchanged.

## 5. Batch and index caps

- [ ] Does this PR add or modify a batch entrypoint (anything taking a
      `Vec<...>` of IDs/addresses) or a bounded index (anything using a
      `MAX_*` constant as an entry-count guard, e.g. `MAX_TRACKED_ADDRESSES`,
      `MAX_BATCH_SIZE`, `MAX_BATCH_EXPIRE`)?
  - If yes: is the size/cap check the **first** thing the function does,
    before any storage read or per-item processing begins? A cap check placed
    after even one item has been processed lets a caller pad an
    otherwise-oversized batch with cheap-to-skip invalid entries to bypass the
    intended per-call limit.
  - Does the guard distinguish "reject a *new* entry once the index is full"
    from "reject an *update to an existing, already-tracked* entry"? Compliance's
    `AddressIndex` cap (issue #48) is deliberately a growth guard, not a
    freeze — operations on already-tracked addresses must keep working even
    when the index is completely full.

## 6. ABI snapshots

- [ ] If this PR changes any contract's public interface (new/removed/renamed
      entrypoint, changed parameter or return type), have the ABI snapshots
      been regenerated per the "ABI snapshots" section of `CONTRIBUTING.md`?

---

Reviewers: treat a `[ ]` you can't confidently check off as a request for
changes, not a nitpick — every item above traces back to a merged fix for a
bug that already happened once in this repo.
