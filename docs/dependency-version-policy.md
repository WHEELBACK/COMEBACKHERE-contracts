# Dependency version policy

## soroban-sdk pin

The workspace pins `soroban-sdk = "22.0.0"` in the root `Cargo.toml` (and the
Stellar CLI used in CI is pinned to `22.8.2` to match its major version).

This pin is deliberate, not an oversight. Dependabot has repeatedly opened
major-version bump PRs against the `soroban-sdk` group (22.0.11 → 26.1.0,
27.0.0, 27.0.1, 27.0.4, 27.0.6) and every one of them has failed CI across the
board — `clippy`, `fmt`, `test`, `test-next-stable`, and `wasm-build` all fail
on the bump branch. The concrete root cause (checked against the CI logs for
PR #357, the 27.0.4 bump attempt): soroban-sdk 27.x deprecates
`Events::publish` in favor of the `#[contractevent]` macro on typed event
structs, and the workspace's CI runs with `-D warnings`, so every one of the
existing `env.events().publish(...)` call sites (used throughout
`contracts/*` and `contracts/settlement-workflow`) turns a deprecation warning
into a hard build failure. That is a real migration — rewriting every emitted
event as a `#[contractevent]` type — not a one-line dependency bump.

### What must be true before attempting the next major bump

1. All `env.events().publish(...)` call sites are migrated to
   `#[contractevent]`-derived types first, on the current 22.x SDK if
   possible, so the bump PR itself doesn't have to carry that migration.
2. A local `cargo clippy -- -D warnings` and `cargo test --workspace` pass
   against the target soroban-sdk version *before* opening the bump PR,
   rather than relying on Dependabot's PR to discover breakage.
3. The Stellar CLI pin in CI is bumped to a version compatible with the new
   SDK major version at the same time (as happened when the CLI moved from
   the no-longer-published 20.0.0 to 22.8.2 alongside the 22.x SDK pin).

Until the event-publishing migration above is done, further Dependabot major
bumps against `soroban-sdk` are expected to keep failing CI for the same
reason and should be closed rather than re-investigated from scratch each
time.
