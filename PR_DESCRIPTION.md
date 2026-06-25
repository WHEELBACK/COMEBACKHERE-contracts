# PR: test(compliance): AlreadyInitialized error on double initialize

## Summary
Adds an explicit test verifying that calling `initialize` twice on the same `ComplianceContract` instance returns `ContractError::AlreadyInitialized`.

## Context
The `initialize` function in `contracts/compliance/src/lib.rs:22-30` already guards against double initialization by checking if the `Admin` storage key exists and returning `Err(ContractError::AlreadyInitialized)` if set. This guard is critical for security — without it, an attacker could re-initialize the contract and seize admin control.

An explicit regression test ensures this guard is not accidentally removed or broken during future refactoring.

## Test added
`reinitialize_is_rejected` in `contracts/compliance/tests/compliance_test.rs:177-182`

- Calls `setup()` which invokes `initialize` once with a legitimate admin.
- Attempts a second `initialize` call via `try_initialize` with a different address.
- Asserts the result is `Err(Ok(ContractError::AlreadyInitialized))`.

## Verification
The test passes under `cargo test --package compliance` (Soroban environment).

---

Closes #77
