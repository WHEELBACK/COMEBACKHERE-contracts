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
