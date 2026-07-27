# Compliance Contract

The Compliance contract manages an allowlist of addresses permitted to interact with the system. It supports permanent and temporary (time-bound) allowing, as well as blocking of addresses.

## Entrypoints

| Function | Auth Required | Parameters | Returns | Errors |
|----------|---------------|------------|---------|--------|
| `initialize` | `admin` | `admin: Address` | `Result<(), ContractError>` | `AlreadyInitialized` |
| `is_allowed` | None | `address: Address` | `bool` | None |
| `allow_address` | `admin` | `admin: Address, address: Address` | `Result<(), ContractError>` | `Unauthorized`, `ContractPaused` |
| `block_address` | `admin` | `admin: Address, address: Address` | `Result<(), ContractError>` | `Unauthorized` |
| `allow_address_until` | `admin` | `admin: Address, address: Address, expires_at: u64` | `Result<(), ContractError>` | `Unauthorized`, `ContractPaused` |
| `transfer_admin` | `admin` | `admin: Address, new_admin: Address` | `Result<(), ContractError>` | `Unauthorized` |
| `accept_admin` | `new_admin` | `new_admin: Address` | `Result<(), ContractError>` | `Unauthorized` |
| `clear_address` | `admin` | `admin: Address, address: Address` | `Result<(), ContractError>` | `Unauthorized` |
| `pause` | `admin` | `admin: Address` | `Result<(), ContractError>` | `Unauthorized` |
| `unpause` | `admin` | `admin: Address` | `Result<(), ContractError>` | `Unauthorized` |

## `is_allowed` precedence

`is_allowed` evaluates state in a fixed order, and **Blocked overrides Allowed**:

1. If the address is `Blocked`, `is_allowed` returns `false` — unless a `BlockedUntil`
   timestamp is set and has passed, in which case the block has auto-expired and
   evaluation falls through to step 2.
2. Otherwise, if the address is not `Allowed`, `is_allowed` returns `false`.
3. Otherwise, if an `AllowedUntil` expiry is set, `is_allowed` returns `true` only
   while the current ledger timestamp is strictly less than that expiry.
4. Otherwise (allowed, no expiry), `is_allowed` returns `true`.

This means an address that is both `Allowed` and `Blocked` is treated as blocked;
`clear_address` must be called to restore it to an allowed state.
