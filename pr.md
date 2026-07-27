## Summary

Implements `address_status` read entrypoint returning full compliance state for an address, and adds two-step admin transfer test coverage.

## Changes

- `allowlist.rs`: Added `AddressStatus` contracttype with fields `allowed`, `blocked`, `expires_at`, `is_currently_allowed`
- `lib.rs`: Exported `AddressStatus`; implemented `address_status(env, address) -> AddressStatus`
- `compliance_test.rs`: Added tests for `address_status` (5 cases) and admin transfer rejection cases (3 cases)

## Tested

- Never-allowed, allowed, blocked, temp-allow active, temp-allow expired states via `address_status`
- Admin transfer happy path, wrong acceptor returns `Unauthorized`, no pending admin panics

Closes #67
Closes #66
