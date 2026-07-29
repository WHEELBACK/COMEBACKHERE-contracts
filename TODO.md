# Implementation Plan - All Complete ✓

## Issue #258 (#67): Add benchmark for approval computation with large signer sets
- [x] Create `contracts/treasury/tests/approval_benchmark_test.rs` with benchmark tests
- [x] Test with signer sets of sizes 10, 50, 100
- [x] Measure approval computation and weight accumulation

## Issue #264 (#73): Document error-code ranges per contract
- [x] Update `ARCHITECTURE.md` with error-code range tables for Invoice, Treasury, Compliance contracts

## Issue #265 (#74): Add append-only enum-ordering CI check
- [x] Create `scripts/check-enum-ordering.sh` script
- [x] Add to `Makefile` as part of `check` target
- [x] Add to `.pre-commit-config.yaml` hooks

## Issue #267 (#76): Add integration test using compliance-client
- [x] Create `contracts/treasury/tests/compliance_client_integration_test.rs`
- [x] Use `ComplianceClient` from `compliance-client` crate
- [x] Add `compliance-client` as dev-dependency in treasury `Cargo.toml`
- [x] Test compliance check in treasury workflow context

