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

Membership is keyed by role *and* address: granting an additional role is a
single small write, and revoking one role cannot disturb any other role.

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
