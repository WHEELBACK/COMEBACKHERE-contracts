# First PR quickstart

The condensed version of [CONTRIBUTING.md](../CONTRIBUTING.md) for a first, small
PR. Read the full document for anything not covered here.

## Before you start

An issue must be assigned to you before you begin work — comment on the issue
to request assignment.

## Setup

```bash
scripts/check-tools.sh   # confirms your local toolchain matches what's pinned
```

## Commands to run before pushing

```bash
cargo fmt --all -- --check
cargo clippy -- -D warnings
cargo test --workspace
```

All three must pass locally before opening a PR — they're exactly what CI
runs.

## Branch and commit

- Branch name: `<type>/<short-description>` in lowercase kebab-case (e.g.
  `docs/quickstart`). See CONTRIBUTING.md for the full prefix list.
- Commit message: [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/),
  with `Closes #<issue_id>` in the footer.

## Opening the PR

Push your branch and open a PR against `main` referencing the issue it
closes. See [CONTRIBUTING.md](../CONTRIBUTING.md) for review expectations and
everything else.
