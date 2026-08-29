# Compliance Contract

The Compliance contract manages an allowlist of addresses permitted to interact with the system. It supports permanent and temporary (time-bound) allowing, as well as blocking of addresses.

## Entrypoints

| Function | Auth Required | Parameters | Returns | Errors |
|----------|---------------|------------|---------|--------|
| `initialize` | `admin` | `admin: Address` | `Result<(), ContractError>` | `AlreadyInitialized` |
| `is_allowed` | None | `address: Address` | `bool` | None |
| `is_blocked` | None | `address: Address` | `bool` | None |
| `allow_address` | `admin` | `admin: Address, address: Address` | `Result<(), ContractError>` | `Unauthorized`, `ContractPaused` |
| `block_address` | `admin` | `admin: Address, address: Address` | `Result<(), ContractError>` | `Unauthorized` |
| `allow_address_until` | `admin` | `admin: Address, address: Address, expires_at: u64` | `Result<(), ContractError>` | `Unauthorized`, `ContractPaused` |
| `allow_address_with_tier` | `admin` | `admin: Address, address: Address, tier: u32` | `Result<(), ContractError>` | `Unauthorized`, `ContractPaused` |
| `get_address_tier` | None | `address: Address` | `u32` | None |
| `set_jurisdiction` | `admin` | `admin: Address, address: Address, jurisdiction_code: Symbol` | `Result<(), ContractError>` | `Unauthorized`, `ContractPaused` |
| `get_jurisdiction` | None | `address: Address` | `Option<Symbol>` | None |
| `transfer_admin` | `admin` | `admin: Address, new_admin: Address` | `Result<(), ContractError>` | `Unauthorized` |
| `accept_admin` | `new_admin` | `new_admin: Address` | `Result<(), ContractError>` | `Unauthorized` |
| `clear_address` | `admin` | `admin: Address, address: Address` | `Result<(), ContractError>` | `Unauthorized` |
| `pause` | `admin` | `admin: Address` | `Result<(), ContractError>` | `Unauthorized` |
| `unpause` | `admin` | `admin: Address` | `Result<(), ContractError>` | `Unauthorized` |

## CLI usage examples

Replace `$COMPLIANCE_CONTRACT`, `$ADMIN`, `$ADDRESS`, and `$NETWORK` with your deployed values.

### initialize

```sh
stellar contract invoke \
  --id $COMPLIANCE_CONTRACT \
  --source $ADMIN \
  --network $NETWORK \
  -- initialize \
  --admin $ADMIN
```

### allow_address

```sh
stellar contract invoke \
  --id $COMPLIANCE_CONTRACT \
  --source $ADMIN \
  --network $NETWORK \
  -- allow_address \
  --admin $ADMIN \
  --address $ADDRESS
```

### is_allowed

```sh
stellar contract invoke \
  --id $COMPLIANCE_CONTRACT \
  --network $NETWORK \
  -- is_allowed \
  --address $ADDRESS
```

---

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

## `is_blocked`

`is_blocked` returns the raw `Blocked` flag for `address`, independent of `is_allowed`.
Unlike `is_allowed`, it does not consult `BlockedUntil` — a block that has auto-expired
by timestamp still reads as `true` here until `clear_address` (or an equivalent state
change) clears the `Blocked` key.

## Compliance tiers

`allow_address_with_tier` allows an address (identically to `allow_address`) and
additionally records a `u32` tier under `DataKey::Tier(address)`. Tier is a bare
numeric convention with no on-chain enforcement:

- `0` — basic KYC (also the default returned by `get_address_tier` for any address
  that has never had a tier set, including addresses allowed via the plain
  `allow_address` entrypoint).
- Higher values are reserved for the caller's own scheme (e.g. `1` = enhanced KYC,
  `2` = institutional). This contract does not interpret tier values beyond storing
  and returning them.

The tier is stored independently of the allow/block state and is **not** consulted
by `is_allowed` — it exists purely as metadata for downstream callers (e.g. the
treasury contract, or an off-chain policy engine) to read via `get_address_tier` and
apply their own tier-based rules (such as differing transaction limits per tier).
Clearing or re-allowing an address via `allow_address`, `clear_address`, etc. does
not reset its stored tier.

## Jurisdiction metadata

`set_jurisdiction` records an optional jurisdiction code (e.g. an ISO 3166 alpha-2
code such as `US` or `EU`) under `DataKey::Jurisdiction(address)`, describing which
regulatory context an address's allow/block determination was made under and is
meant to apply to. Like tiers, this is pure metadata:

- It does not affect `is_allowed`, `is_blocked`, or any other compliance-gate logic
  in this contract — a jurisdiction-aware policy must be enforced by the caller.
- `get_jurisdiction` returns `None` for any address that has never had a jurisdiction
  set, including every address tracked before this field existed — there is no
  default jurisdiction and no migration is required for existing data.
- Setting a new jurisdiction code overwrites any previously stored value for that
  address; there is no history of prior jurisdiction assignments.
