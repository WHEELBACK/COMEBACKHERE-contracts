# COMEBACKHERE-contracts

> Rust/Soroban smart contracts for COMEBACKHERE Protocol.

This repository owns invoice escrow state, payment validation, multi-sig treasury settlement, and compliance gates.

## Workspace

- `contracts/invoice` — invoice state machine and payment marking
- `contracts/treasury` — 2-of-3 settlement approval workflow
- `contracts/compliance` — admin-managed allow/block list

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — contract design, DataKey storage reference, and cross-contract call map
- [GLOSSARY.md](GLOSSARY.md) — definitions for protocol terms used across contracts and docs
- [SECURITY.md](SECURITY.md) — vulnerability reporting and disclosure policy
- [CONTRIBUTING.md](CONTRIBUTING.md) — branch, commit, and PR guidelines

## Development

```sh
# Format code
just fmt
# or
cargo fmt --all

# Run lints
just lint
# or
cargo clippy -- -D warnings

# Run tests
just test
# or
cargo test

# Run all checks
just check
```

## ABI Snapshots

After changing contract interfaces, regenerate ABI metadata from the sibling `COMEBACKHERE/` repo:

```sh
cd ../COMEBACKHERE
make update-abi-snapshots
```

## Toolchain

- **Rust**: `1.95.0` (see `rust-toolchain.toml`)
- **Target**: `wasm32-unknown-unknown`
- **Stellar CLI**: `20.0.0`

Verify your setup by running:
```sh
./scripts/check-tools.sh
```

## License

MIT
