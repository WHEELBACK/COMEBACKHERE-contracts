---
name: Feature request
about: Suggest an idea for this project
title: ""
labels: enhancement
assignees: ""
---

## Description
A clear and concise description of what you want to happen.

## Requirements and context
Explain the motivation, edge cases, and any relevant background. Mention
related issues or prior art if applicable.

## Suggested execution
Outline the implementation approach, files likely to change, and any new
dependencies or contracts that may be introduced.

## Test and commit
- [ ] Unit tests covering the new functionality
- [ ] Cross-contract integration tests if the feature touches more than one contract
- [ ] `cargo test --all` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --all -- --check` passes

## Guidelines
- Follow the existing code style and patterns in the codebase.
- Keep contracts `#![no_std]` compatible.
- Do not introduce panic paths where `Result`-based error handling exists.
- All new public functions must emit events.
