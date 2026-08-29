# Contributor FAQ

This is a living reference for contributors to the `COMEBACKHERE-contracts`
repository. The answers below describe this repository's current commands and
process; they are intentionally specific rather than a general Rust or
Soroban guide.

## Setup and tooling

### Which Rust toolchain and target should I install?

Use Rust `1.95.0` with the `wasm32-unknown-unknown` target. The pinned
toolchain is in `rust-toolchain.toml`, and the test workflow uses the same
toolchain and target. Rustfmt and Clippy are also listed as required
components. Run `./scripts/check-tools.sh` to verify the Rust version and
target before debugging a setup problem.

### Which Stellar CLI version does this repository require?

Use Stellar CLI `20.0.0`. `scripts/check-tools.sh` checks `stellar --version`
and rejects another version. Install or update the exact version with:

```sh
cargo install --locked stellar-cli --version 20.0.0
```

The CLI is used by the deployment recipe in `justfile`; it is separate from
the Rust toolchain used to compile and test the contracts.

### Why does the first `cargo test --all` take much longer?

The first test run on a fresh clone has to download dependencies and compile
the workspace, including Soroban SDK code and test utilities. Later runs can
reuse Cargo's registry, Git, and `target/` build artifacts, so they are usually
much faster. A long initial compile is expected; check the command's final
exit status before treating it as a failure. CI likewise runs the complete
workspace test command (`cargo test --workspace`).

### How do I run only one contract's tests?

Use the package name from that contract's `Cargo.toml`:

```sh
cargo test -p comebackhere-compliance
cargo test -p comebackhere-invoice
cargo test -p comebackhere-treasury
```

Add a test name to narrow it further, for example
`cargo test -p comebackhere-invoice test_expiry_overflow_rejected`. These are
package names, not the shorter library names shown in each `[lib]` section.

### How do I run the cross-contract integration tests?

The integration-test crate is named `comebackhere-tests` in `tests/Cargo.toml`.
Run it with:

```sh
cargo test -p comebackhere-tests
```

The tests cover invoice-to-treasury, treasury-to-compliance, and full
invoice-to-settlement flows. If a command copied from an older comment uses
`protocol-integration-tests`, use the package name in `tests/Cargo.toml`
instead.

### Where does the repository enable integer overflow checks?

There is currently no explicit `overflow-checks = true` profile setting in
the tracked Cargo configuration. Normal `cargo test` builds use Cargo's debug
profile, where Rust arithmetic overflow checks are enabled by default; release
WASM builds have different profile behavior. Overflow remains a deliberate
security concern here: boundary tests cover invoice ID and amount arithmetic,
and `SECURITY.md` lists integer overflow and underflow in payment or
settlement math as in scope. Do not remove or weaken those tests when changing
arithmetic.

## Contribution process

### What does the `Stellar Wave` label mean?

On this repository, `Stellar Wave` is the label for issues in the Stellar wave
program. It identifies the program context; it does not replace the normal
technical acceptance criteria, review, or linked-issue requirement.

### Do I need assignment before I start, and what is the timeframe?

Yes. `CONTRIBUTING.md` says an issue must be assigned before work begins, so
comment on the issue to request assignment first. For the current bounty task,
the stated timeframe is 24 hours. Treat the issue's assignment and timeframe
as the controlling program instructions, and ask in the issue if either is
unclear before starting implementation.

### What branch and PR format should I use?

Use a lowercase kebab-case branch named `<type>/<short-description>`, such as
`docs/contributor-faq`. Documentation-only work uses the `docs/` prefix. PRs
must include a description, link the issue with `Closes #<issue_id>`, and have
at least one approving review. The project squash-merges into `main`, and
commit messages must follow Conventional Commits.

### Which checks should I run before opening the PR?

Run the same focused quality gates enforced by the repository configuration:

```sh
cargo test --all
cargo clippy -- -D warnings
cargo fmt --all -- --check
```

The CI workflows express the equivalent workspace forms (`--workspace` for
tests and Clippy). `pre-commit run --all-files` also runs the format and Clippy
hooks when pre-commit is installed. The `just check` recipe runs formatting,
Clippy, and tests locally.

### When do I need to regenerate ABI snapshots?

Only contract interface changes require ABI snapshot work. After changing a
contract's public interface, use the sibling `COMEBACKHERE/` repository and
run `make update-abi-snapshots` (or `just snapshot` there), as described in
`CONTRIBUTING.md`. A documentation-only change in this repository does not
need ABI regeneration.

### How do I build the deployable contract artifacts?

Install the pinned Rust target, then run:

```sh
cargo build --target wasm32-unknown-unknown --release
```

The output is written under
`target/wasm32-unknown-unknown/release/`. The CI build checks each of the
`comebackhere-compliance`, `comebackhere-invoice`, and
`comebackhere-treasury` packages as WASM. Deployment through `just deploy`
also requires `STELLAR_ACCOUNT` and `STELLAR_NETWORK`.

## Keeping this FAQ useful

Add a question when contributors encounter a recurring, repository-specific
setup or process problem. Verify the answer against the current files,
workflows, and scripts before updating this document.
