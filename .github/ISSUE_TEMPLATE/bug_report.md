---
name: Bug report
about: Create a report to help us improve
title: ""
labels: bug
assignees: ""
---

## Description
A clear and concise description of the bug.

## Requirements and context
What is the expected behavior? What actually happens? Include relevant
details about the environment, contract state, and invocation parameters.

## Steps to reproduce
1. Deploy contract(s) '...'
2. Call function '...' with parameters '...'
3. See error / unexpected result

## Suggested execution
If known, describe the root cause and a proposed fix. Identify the files
and functions involved.

## Test and commit
- [ ] A regression test that reproduces the bug
- [ ] `cargo test --all` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --all -- --check` passes

## Guidelines
- The fix must not introduce new panic paths where `Result`-based error
  handling already exists in the surrounding code.
- Preserve the `#![no_std]` constraint.
- Add or update event emissions if the fix changes contract state.
