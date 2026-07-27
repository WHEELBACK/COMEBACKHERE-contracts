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
