# Contributor FAQ Implementation Report

## Date

2026-08-27

## Branch

`COMEBACKHERE-contracts431`

## Work completed

Created [`docs/FAQ.md`](FAQ.md), a living contributor FAQ containing 12
repository-specific questions and answers.

The FAQ documents:

- Rust `1.95.0` and the `wasm32-unknown-unknown` target
- Stellar CLI `20.0.0` and the repository's version-check script
- Why the first workspace test run takes longer
- How to run one contract's tests with Cargo package names
- How to run cross-contract integration tests
- The repository's current overflow-check configuration and security tests
- The meaning of the `Stellar Wave` label
- Assignment requirements and the 24-hour bounty timeframe
- Branch naming, PR requirements, and Conventional Commits
- Required quality checks, ABI snapshots, and WASM builds

## Sources verified

Answers were checked against the current repository files, including:

- `rust-toolchain.toml`
- `Cargo.toml` and package manifests
- `.github/workflows/`
- `scripts/check-tools.sh`
- `CONTRIBUTING.md`
- `README.md`
- `SECURITY.md`
- `justfile`

The FAQ explicitly records that no tracked Cargo configuration currently sets
`overflow-checks = true`; test builds rely on debug-profile overflow behavior.

## Validation

Passed:

- Markdown file error check
- Diff whitespace check
- ASCII-content check
- FAQ entry count check: 12 entries

Not run successfully:

- `cargo test --all`
- `cargo clippy -- -D warnings`
- `cargo fmt --all -- --check`

The container does not have `cargo` installed or available on `PATH`, so these
Rust commands could not be executed.

## Commits

- `7c2573e` - `docs(docs): add contributor FAQ document`
- `9661748` - `test: update full lifecycle integration test`

The FAQ branch was pushed to `origin/COMEBACKHERE-contracts431`.
