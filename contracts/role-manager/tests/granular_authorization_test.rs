//! #933: granular role-based authorization for parameter changes.
//!
//! A single "is admin?" gate cannot express that a compliance officer may set
//! tier limits but not settlement thresholds. Every parameter mutation now
//! names a [`Permission`], and the role → permission matrix is a single static
//! function in the contract. Tests cover the full matrix, the absence of an
//! admin bypass, enforcement from a consumer contract, and the cross-contract
//! write to the contract that actually owns the parameter.

use proptest::prelude::*;
use role_manager::{
    Permission, Role, RoleManagerClient, RoleManagerContract, RoleManagerContractClient,
    RoleManagerError,
};
use soroban_sdk::{
    contract, contractimpl, panic_with_error, testutils::Address as _, Address, Env, Symbol,
};

const ROLES: [Role; 5] = [
    Role::Admin,
    Role::Operator,
    Role::ComplianceOfficer,
    Role::TreasurySigner,
    Role::Pauser,
];
const PERMISSIONS: [Permission; 5] = [
    Permission::ManageRoles,
    Permission::SetThreshold,
    Permission::SetSignerWeight,
    Permission::SetTierLimit,
    Permission::PauseProtocol,
];

fn expected(role: Role, permission: Permission) -> bool {
    matches!(
        (role, permission),
        (Role::Admin, Permission::ManageRoles)
            | (Role::Operator, Permission::SetThreshold)
            | (Role::ComplianceOfficer, Permission::SetTierLimit)
            | (Role::TreasurySigner, Permission::SetSignerWeight)
            | (Role::Pauser, Permission::PauseProtocol)
    )
}

/// Stand-in for a protocol contract that owns a parameter.
#[contract]
struct ParameterTarget;

#[contractimpl]
impl ParameterTarget {
    pub fn set_parameter(env: Env, key: Symbol, value: u32) {
        if env.storage().instance().has(&Symbol::new(&env, "reject")) {
            panic_with_error!(&env, RoleManagerError::PermissionDenied);
        }
        env.storage().instance().set(&key, &value);
    }

    pub fn get_parameter(env: Env, key: Symbol) -> u32 {
        env.storage().instance().get(&key).unwrap_or(0)
    }

    pub fn set_reject(env: Env, reject: bool) {
        let key = Symbol::new(&env, "reject");
        if reject {
            env.storage().instance().set(&key, &true);
        } else {
            env.storage().instance().remove(&key);
        }
    }
}

/// Consumer that guards its own setter with the granular permission check.
#[contract]
struct GuardedConsumer;

#[contractimpl]
impl GuardedConsumer {
    pub fn set_threshold(env: Env, role_manager: Address, caller: Address, value: u32) {
        RoleManagerClient::new(&env, &role_manager)
            .require_permission(&caller, &Permission::SetThreshold);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "threshold"), &value);
    }

    pub fn threshold(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "threshold"))
            .unwrap_or(0)
    }
}

struct Context {
    env: Env,
    admin: Address,
    role_manager: Address,
    holders: std::vec::Vec<Address>,
    stranger: Address,
}

fn setup() -> Context {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let role_manager = env.register(RoleManagerContract, ());
    let client = RoleManagerContractClient::new(&env, &role_manager);
    client.initialize(&admin);

    let holders = ROLES
        .iter()
        .map(|role| {
            let holder = Address::generate(&env);
            client.grant_role(&admin, role, &holder);
            holder
        })
        .collect();
    let stranger = Address::generate(&env);

    Context {
        env,
        admin,
        role_manager,
        holders,
        stranger,
    }
}

#[test]
fn full_role_permission_matrix_is_enforced() {
    let ctx = setup();
    let client = RoleManagerContractClient::new(&ctx.env, &ctx.role_manager);

    for (role, holder) in ROLES.iter().zip(ctx.holders.iter()) {
        for permission in PERMISSIONS.iter() {
            let target = ctx.env.register(ParameterTarget, ());
            let target_client = ParameterTargetClient::new(&ctx.env, &target);
            let key = Symbol::new(&ctx.env, "param");

            let result =
                client.try_apply_parameter_change(holder, permission, &target, &key, &7_u32);
            if expected(*role, *permission) {
                assert!(result.is_ok(), "{role:?} should permit {permission:?}");
                assert_eq!(target_client.get_parameter(&key), 7);
            } else {
                assert_eq!(
                    result,
                    Err(Ok(RoleManagerError::PermissionDenied)),
                    "{role:?} must not permit {permission:?}"
                );
                assert_eq!(target_client.get_parameter(&key), 0);
            }
        }
    }
}

#[test]
fn admin_role_does_not_bypass_parameter_permissions() {
    let ctx = setup();
    let client = RoleManagerContractClient::new(&ctx.env, &ctx.role_manager);
    let target = ctx.env.register(ParameterTarget, ());
    let target_client = ParameterTargetClient::new(&ctx.env, &target);
    let key = Symbol::new(&ctx.env, "threshold");

    assert_eq!(
        client.try_apply_parameter_change(
            &ctx.admin,
            &Permission::SetThreshold,
            &target,
            &key,
            &99
        ),
        Err(Ok(RoleManagerError::PermissionDenied))
    );
    assert_eq!(target_client.get_parameter(&key), 0);

    // The admin keeps the one permission it does own.
    assert!(client.has_permission(&ctx.admin, &Permission::ManageRoles));
}

#[test]
fn address_without_a_role_cannot_change_a_parameter() {
    let ctx = setup();
    let client = RoleManagerContractClient::new(&ctx.env, &ctx.role_manager);
    let target = ctx.env.register(ParameterTarget, ());
    let target_client = ParameterTargetClient::new(&ctx.env, &target);
    let key = Symbol::new(&ctx.env, "threshold");

    assert_eq!(
        client.try_apply_parameter_change(
            &ctx.stranger,
            &Permission::SetThreshold,
            &target,
            &key,
            &99
        ),
        Err(Ok(RoleManagerError::PermissionDenied))
    );
    assert_eq!(target_client.get_parameter(&key), 0);
}

#[test]
fn consumer_contract_enforces_permission_before_its_own_write() {
    let ctx = setup();
    let consumer = ctx.env.register(GuardedConsumer, ());
    let guarded = GuardedConsumerClient::new(&ctx.env, &consumer);

    guarded.set_threshold(&ctx.role_manager, &ctx.holders[1], &5);
    assert_eq!(guarded.threshold(), 5);

    // Compliance officer is real and authenticated, but has no SetThreshold.
    // The consumer's () returning client traps, so the host surfaces a failed
    // invocation rather than a typed contract error across the boundary.
    assert!(guarded
        .try_set_threshold(&ctx.role_manager, &ctx.holders[2], &50)
        .is_err());
    assert_eq!(guarded.threshold(), 5);
}

#[test]
fn permission_check_authenticates_caller_in_a_sub_invocation() {
    let ctx = setup();
    let consumer = ctx.env.register(GuardedConsumer, ());
    let guarded = GuardedConsumerClient::new(&ctx.env, &consumer);
    let auths_before = ctx.env.auths().len();

    guarded.set_threshold(&ctx.role_manager, &ctx.holders[1], &1);

    let newly_authorized: std::vec::Vec<Address> = ctx
        .env
        .auths()
        .into_iter()
        .skip(auths_before)
        .map(|(address, _)| address)
        .collect();
    assert!(newly_authorized.contains(&ctx.holders[1]));
}

#[test]
fn target_failure_is_typed_and_rolls_back() {
    let ctx = setup();
    let client = RoleManagerContractClient::new(&ctx.env, &ctx.role_manager);
    let target = ctx.env.register(ParameterTarget, ());
    let target_client = ParameterTargetClient::new(&ctx.env, &target);
    let key = Symbol::new(&ctx.env, "threshold");

    target_client.set_reject(&true);
    assert_eq!(
        client.try_apply_parameter_change(
            &ctx.holders[1],
            &Permission::SetThreshold,
            &target,
            &key,
            &1
        ),
        Err(Ok(RoleManagerError::ParameterTargetRejected))
    );
    assert_eq!(target_client.get_parameter(&key), 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// The published `has_permission` query and the enforcing
    /// `apply_parameter_change` entrypoint must agree for every role/permission
    /// pair — no hidden bypass in either direction.
    #[test]
    fn prop_permission_matrix_matches_enforcement(role_idx in 0usize..5, permission_idx in 0usize..5) {
        let ctx = setup();
        let client = RoleManagerContractClient::new(&ctx.env, &ctx.role_manager);
        let role = ROLES[role_idx];
        let permission = PERMISSIONS[permission_idx];
        let holder = &ctx.holders[role_idx];
        let should_allow = expected(role, permission);

        prop_assert_eq!(client.has_permission(holder, &permission), should_allow);

        let target = ctx.env.register(ParameterTarget, ());
        let result = client.try_apply_parameter_change(
            holder,
            &permission,
            &target,
            &Symbol::new(&ctx.env, "param"),
            &1,
        );
        if should_allow {
            prop_assert!(result.is_ok());
        } else {
            prop_assert_eq!(result, Err(Ok(RoleManagerError::PermissionDenied)));
        }
    }
}
