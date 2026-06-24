# PR: feat(compliance): add revoke_allow entrypoint for soft de-listing

## Summary
Adds `revoke_allow(env, admin, address)` — a new entrypoint that removes the `Allowed` flag and any `AllowedUntil` expiry from an address without setting the `Blocked` flag. This enables operators to de-list an address from the allowlist without placing it on the blocklist.

## Motivation
Previously, the only way to remove an address from the allowlist was:
- `block_address` — sets `Blocked = true`, marking the address as malicious
- `clear_address` — sets `Blocked = false, Allowed = true`, which re-allows

Neither provides a clean "remove from allowlist but don't accuse" path. `revoke_allow` fills this gap for addresses that are no longer active but not suspected of wrongdoing.

## Implementation
- **`contracts/compliance/src/lib.rs`**: New `revoke_allow` function in `ComplianceContract`:
  - Authenticates the admin via `require_admin`
  - Checks the contract is not paused via `require_not_paused`
  - Removes `DataKey::Allowed` and `DataKey::AllowedUntil` from persistent storage
  - Tracks the address (so `export_snapshot` captures the state transition)
  - Emits an `address_revoked` event

## Tests added
All in `contracts/compliance/tests/compliance_test.rs`:

| Test | Description |
|------|-------------|
| `revoke_allow_removes_allowed_status` | Allow → revoke → `is_allowed` returns `false` |
| `revoke_allow_does_not_block` | After revoke, re-allow succeeds (blocked flag was not set) |
| `revoke_allow_removes_expiry` | Temp allow → revoke → `is_allowed` returns `false`, can re-allow |
| `revoke_allow_returns_unauthorized_for_non_admin` | Non-admin gets `ContractError::Unauthorized` |
| `revoke_allow_returns_contract_paused_when_paused` | When paused, returns `ContractError::ContractPaused` |

## Verification
All 40 tests pass: `cargo test --package comebackhere-compliance`

---

Closes #76
