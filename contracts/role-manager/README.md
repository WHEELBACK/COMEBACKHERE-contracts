# Role Manager

On-chain role registry for the COMEBACKHERE protocol. It is the single place a
consumer contract checks whether an address is allowed to perform an operation,
instead of every contract maintaining its own divergent role map.

> Status: isolated registry. Existing production contracts still use their own
> local admin/operator/signer checks; this contract exposes the interface they
> can migrate to.

## Storage

| Key | Durability | Value | Purpose |
|---|---|---|---|
| `Admin` | instance | `Address` | Administrator allowed to grant/revoke roles |
| `RoleMember(Role, Address)` | persistent | `bool` | Role membership; absent means not held |
| `Budget(Role, Address)` | persistent | `i128` | Non-negative action budget; absent means zero |

## Granular permissions

Parameter setters should check a `Permission`, not "is this address an admin?".
The static matrix is:

| Role | Permission |
|---|---|
| `Admin` | `ManageRoles` |
| `Operator` | `SetThreshold` |
| `ComplianceOfficer` | `SetTierLimit` |
| `TreasurySigner` | `SetSignerWeight` |
| `Pauser` | `PauseProtocol` |

`Admin` deliberately does **not** inherit every permission: leaking an admin key
must not let its holder rewrite unrelated protocol parameters. New roles are
added to one `ALL_ROLES` array so a forgotten role cannot silently escape the
permission scan.

Membership is keyed by role *and* address: granting an additional role is a
single small write, and revoking one role cannot disturb any other role.
Budgets use the same key shape, so a budget is always scoped to one role and
cannot be spent through a different role.

## Roles

| Role | Intended use |
|---|---|
| `Admin` | Manage the registry itself |
| `Operator` | Day-to-day operational parameter changes |
| `ComplianceOfficer` | Compliance parameter changes |
| `TreasurySigner` | Treasury signing parameter changes |
| `Pauser` | Emergency pause operations |

## Entrypoints

| Entrypoint | Auth | Notes |
|---|---|---|
| `initialize(admin)` | self-auth of `admin` | One-shot; also grants `Admin` to `admin` |
| `grant_role(admin, role, member)` | admin | Writes `true` under `RoleMember(role, member)` |
| `revoke_role(admin, role, member)` | admin | Writes `false`; revocation is explicit and queryable |
| `has_role(role, member)` | permissionless read | Returns `false` for absent and revoked memberships |
| `require_role(caller, role)` | self-auth of `caller` | Fails with `RoleNotAssigned` when the role is absent |
| `get_budget(role, member)` | permissionless read | Returns `0` for a missing or exhausted budget |
| `credit_budget(admin, role, member, amount)` | admin | Adds to a budget; `0` is a storage-free no-op |
| `debit_budget(caller, role, amount)` | self-auth of `caller` + role | Spends the caller's own budget; removes the key at zero |
| `has_permission(address, permission)` | permissionless read | True when any held role grants the permission |
| `require_permission(caller, permission)` | self-auth of `caller` | Fails with `PermissionDenied` when no role grants it |
| `apply_parameter_change(caller, permission, target, key, value)` | self-auth + permission | Checks first, then calls `target.set_parameter`; rolls back on rejection |

### TTL maintenance

Every persistent read/write goes through one of two helpers that call
`extend_ttl` with a threshold of 4,096 ledgers and a target of 10,000 ledgers.
The host skips the write while a key is healthy, so hot authorization lookups
do not pay for a rent top-up on every access. State-changing entrypoints also
refresh the contract instance so its admin pointer cannot expire while the
persistent entries it controls are alive. See `tests/ttl_bump_test.rs` for the
exact TTL and ledger-jump assertions.

### Zero-balance behaviour

An absent budget is a valid `0`, not a storage error. `debit_budget` on such a
balance returns `InsufficientBudget` (a typed contract error, never a host
panic), a zero-amount credit/debit writes nothing, and an exact debit removes
the key entirely. Negative amounts return `InvalidAmount` and credit overflow
returns `BudgetOverflow`, both without mutating state.

## Cross-contract use

Consumers depend only on the thin `RoleManagerClient` generated from
`RoleManagerInterface` (declared in `src/lib.rs`), so they do not link the role
manager's own wasm exports:

```rust
RoleManagerClient::new(&env, &role_manager_id).require_role(&caller, &Role::Operator);
```

`require_role` authenticates `caller` inside the sub-invocation. In testutils,
use `env.mock_all_auths_allowing_non_root_auth()` for tests that go through a
consumer contract; plain `mock_all_auths()` only covers root-invocation auth.

`tests/cross_contract_invocation_test.rs` exercises the real contract, a mock
implementing the same client surface, denial propagation, and the auth tree,
all through Soroban SDK testutils.
