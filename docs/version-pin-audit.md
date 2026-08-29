# Version Pin Audit

One-time audit of every pinned version across `.github/workflows/*.yml`, `README.md`, and `rust-toolchain.toml`, prompted by the stellar-cli 20.0.0 staleness that went undetected in two workflow files at once (see #issues around the CI-fix pass that caught it). This is an audit-and-report pass: pins are only bumped here if confirmed stale or broken; everything else is left as-is.

## Findings

| Pin | Location | Status |
| --- | --- | --- |
| `rust-toolchain.channel = "1.95.0"` | `rust-toolchain.toml` | Current. Matches the `1.95.0` toolchain pinned in every workflow (`fmt.yml`, `build.yml`, `contract-size.yml`, `lint.yml`, `init-smoke-test.yml`, `test.yml`). Consistent across all files. |
| `STELLAR_CLI_VERSION = "22.8.2"` | `build.yml`, `init-smoke-test.yml` | Current and consistent between both files. (This is the pin that was previously stale at `20.0.0`; it has since been corrected.) |
| README.md toolchain table (Rust `1.95.0`, Stellar CLI `22.8.2`) | `README.md` | Matches `rust-toolchain.toml` and the workflow pins above. No drift. |
| `actions/checkout@v4` | all workflows | Still a supported major version, but pinned to a Node 20 runtime that GitHub has begun deprecating (Node 20 removal target: September 2026). `actions/checkout` maintainers are expected to migrate v4 to Node 24 under the same tag, so this is not currently broken, but is worth re-checking closer to the Node 20 removal date. Not bumped here since nothing is confirmed broken yet. |
| `dtolnay/rust-toolchain@master` / `@stable` | most workflows | These are floating refs by design (the action has no versioned tags), not version pins, so "staleness" doesn't apply the same way. No action needed. |
| `Swatinem/rust-cache@v2` | `build.yml`, `lint.yml`, `test.yml` | Current major version, functioning as expected in CI. No drift found. |
| `actions/cache@v4` | `contract-size.yml`, `init-smoke-test.yml` | Current major version. No drift found. |
| `codecov/codecov-action@v4` | `coverage.yml` | Confirmed several major versions behind upstream (v6/v7 have since shipped). Not confirmed broken — coverage upload is not part of this repo's required merge checks — but this is a real gap worth a dedicated follow-up rather than a speculative bump bundled into an unrelated audit PR. |
| `EmbarkStudios/cargo-deny-action@v2` | `deny.yml` | Current major version. No drift found. |
| `actions/github-script@v7` | `deny.yml` | Current major version. No drift found. |

## Follow-up

- `codecov/codecov-action@v4` → `v7`: recommend a small, isolated PR bumping this pin and confirming coverage upload still succeeds, since it's a real (if non-blocking) staleness gap. Not bundled into this audit PR to keep the diff to confirmed-stale changes only, per this issue's scope.

No other pins were found stale or broken as of this audit (2026-08-28).
