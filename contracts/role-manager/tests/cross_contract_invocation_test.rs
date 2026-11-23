//! #940: cross-contract invocation tests for the role manager.
//!
//! A role registry is only useful when consumers actually call it. This suite
//! exercises the *call boundary*, not just the contract in isolation:
//!
//! - a real `RoleManagerContract` registered with `Env::register_contract` and
//!   invoked through the generated client by a consumer contract;
//! - a mock role manager implementing the same `RoleManagerInterface` client
//!   surface, swapped in without changing the consumer contract at all;
//! - denial propagation — a rejected role check must abort the consumer's
//!   parameter write, leaving no partially applied state;
//! - authentication — the checked address must actually authorize the nested
//!   cross-contract call, not merely be listed as a role holder.
//!
//! Everything runs on Soroban SDK testutils (`Env::default`,
//! `env.register_contract`, generated clients), so the same harness can later
//! be pointed at a WASM registration with no test changes.

use role_manager::{
    Role, RoleManagerClient, RoleManagerContract, RoleManagerContractClient, RoleManagerError,
};
use soroban_sdk::{
    contract, contractimpl, panic_with_error, testutils::Address as _, Address, Env, IntoVal,
    Symbol, Vec as SorobanVec,
};

// ---------------------------------------------------------------------------
// Consumer under test: a contract that guards a parameter write with a role.
// ---------------------------------------------------------------------------

#[contract]
struct ParameterConsumer;

#[contractimpl]
impl ParameterConsumer {
    /// Writes `value` only when the role manager accepts `caller` as an operator.
    pub fn set_threshold(env: Env, role_manager: Address, caller: Address, value: u32) {
        RoleManagerClient::new(&env, &role_manager).require_role(&caller, &Role::Operator);
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

// ---------------------------------------------------------------------------
// Mock role manager: same client surface, scripted decision + call counter.
// ---------------------------------------------------------------------------

#[contract]
struct MockRoleManager;

#[contractimpl]
impl MockRoleManager {
    pub fn initialize(env: Env, expected: Address) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "expected"), &expected);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "calls"), &0_u32);
    }

    /// Mirrors `RoleManagerContract::require_role` for the consumer's client.
    pub fn require_role(env: Env, caller: Address, role: Role) {
        let calls: u32 = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "calls"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "calls"), &(calls + 1));

        let expected: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "expected"))
            .unwrap();
        if caller != expected || role != Role::Operator {
            panic_with_error!(&env, RoleManagerError::RoleNotAssigned);
        }
    }

    pub fn calls(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "calls"))
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct Context {
    env: Env,
    operator: Address,
    intruder: Address,
    role_manager: Address,
    consumer: Address,
}

fn real_context() -> Context {
    let env = Env::default();
    // The consumer contract calls the role manager as a sub-invocation, and
    // `require_role` authenticates an address passed in by that consumer. Plain
    // `mock_all_auths` only records root-invocation auth, so nested auth must be
    // explicitly allowed here (this is also how production auth trees behave:
    // a sub-invocation's `require_auth` is attributed to the parent).
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let operator = Address::generate(&env);
    let intruder = Address::generate(&env);

    let role_manager = env.register(RoleManagerContract, ());
    let rm = RoleManagerContractClient::new(&env, &role_manager);
    rm.initialize(&admin);
    rm.grant_role(&admin, &Role::Operator, &operator);

    let consumer = env.register(ParameterConsumer, ());

    Context {
        env,
        operator,
        intruder,
        role_manager,
        consumer,
    }
}

/// Invoke the consumer through the host directly so the typed error raised
/// inside the nested role-manager call is observable to the test.
#[allow(clippy::too_many_arguments)]
fn try_set_threshold(
    env: &Env,
    consumer: &Address,
    role_manager: &Address,
    caller: &Address,
    value: u32,
) -> Result<
    Result<(), soroban_sdk::ConversionError>,
    Result<RoleManagerError, soroban_sdk::InvokeError>,
> {
    env.try_invoke_contract::<(), RoleManagerError>(
        consumer,
        &Symbol::new(env, "set_threshold"),
        SorobanVec::from_array(
            env,
            [
                role_manager.into_val(env),
                caller.into_val(env),
                value.into_val(env),
            ],
        ),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn real_role_manager_authorizes_consumer_parameter_write() {
    let ctx = real_context();
    let consumer = ParameterConsumerClient::new(&ctx.env, &ctx.consumer);

    consumer.set_threshold(&ctx.role_manager, &ctx.operator, &42);

    assert_eq!(consumer.threshold(), 42);
    assert!(RoleManagerContractClient::new(&ctx.env, &ctx.role_manager)
        .has_role(&Role::Operator, &ctx.operator));
}

#[test]
fn denial_propagates_across_contracts_and_leaves_no_parameter_write() {
    let ctx = real_context();
    let consumer = ParameterConsumerClient::new(&ctx.env, &ctx.consumer);

    assert!(matches!(
        try_set_threshold(
            &ctx.env,
            &ctx.consumer,
            &ctx.role_manager,
            &ctx.intruder,
            99,
        ),
        Err(Ok(RoleManagerError::RoleNotAssigned))
    ));

    // The role check runs before the write, so the failed invocation must not
    // leave the consumer's parameter changed.
    assert_eq!(consumer.threshold(), 0);
}

#[test]
fn mock_role_manager_is_interchangeable_with_the_real_one() {
    let ctx = real_context();
    let consumer = ParameterConsumerClient::new(&ctx.env, &ctx.consumer);

    let mock = ctx.env.register(MockRoleManager, ());
    let mock_client = MockRoleManagerClient::new(&ctx.env, &mock);
    mock_client.initialize(&ctx.operator);

    // Same consumer entrypoint, different role-manager address: the mock is
    // exercised over the identical cross-contract client surface.
    consumer.set_threshold(&mock, &ctx.operator, &7);
    assert_eq!(consumer.threshold(), 7);
    assert_eq!(mock_client.calls(), 1);

    assert!(matches!(
        try_set_threshold(&ctx.env, &ctx.consumer, &mock, &ctx.intruder, 100),
        Err(Ok(RoleManagerError::RoleNotAssigned))
    ));
    assert_eq!(consumer.threshold(), 7);
    // The mock incremented its counter before rejecting, but Soroban rolls the
    // whole failed sub-invocation tree back: the caller sees the typed error and
    // none of the mock's writes survive. This is the cross-contract atomicity
    // guarantee consumers rely on when a role check aborts a parameter write.
    assert_eq!(mock_client.calls(), 1);
}

#[test]
fn cross_contract_role_check_authenticates_the_checked_address() {
    let ctx = real_context();
    let consumer = ParameterConsumerClient::new(&ctx.env, &ctx.consumer);
    let auths_before = ctx.env.auths().len();

    consumer.set_threshold(&ctx.role_manager, &ctx.operator, &11);

    let newly_authorized: std::vec::Vec<Address> = ctx
        .env
        .auths()
        .into_iter()
        .skip(auths_before)
        .map(|(address, _)| address)
        .collect();
    assert!(newly_authorized.contains(&ctx.operator));
}
