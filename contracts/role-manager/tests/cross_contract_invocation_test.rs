//! Cross-contract invocation mock tests for `role-manager` (#940).

use role_manager::{RoleManagerContract, RoleManagerContractClient, RoleManagerError};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, testutils::Address as _, Address, Env,
    Symbol,
};

#[contracttype]
#[derive(Clone)]
enum ConsumerKey {
    RoleManager,
    Value,
}

#[contract]
pub struct ParameterConsumer;

#[contractimpl]
impl ParameterConsumer {
    pub fn init(env: Env, role_manager: Address) {
        env.storage()
            .instance()
            .set(&ConsumerKey::RoleManager, &role_manager);
        env.storage().instance().set(&ConsumerKey::Value, &0u64);
    }

    pub fn update_value(env: Env, caller: Address, new_value: u64) -> Result<(), RoleManagerError> {
        let rm: Address = env
            .storage()
            .instance()
            .get(&ConsumerKey::RoleManager)
            .expect("consumer not initialized");
        let client = RoleManagerContractClient::new(&env, &rm);
        client
            .try_require_role(&symbol_short!("operator"), &caller)
            .map_err(|e| e.unwrap_or(RoleManagerError::MissingRole))?
            .map_err(|_| RoleManagerError::MissingRole)?;
        env.storage()
            .instance()
            .set(&ConsumerKey::Value, &new_value);
        Ok(())
    }

    pub fn get_value(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&ConsumerKey::Value)
            .unwrap_or(0)
    }
}

#[contract]
pub struct MockRoleManager;

#[contractimpl]
impl MockRoleManager {
    pub fn require_role(
        _env: Env,
        _role: Symbol,
        _account: Address,
    ) -> Result<(), RoleManagerError> {
        Err(RoleManagerError::MissingRole)
    }
}

#[test]
fn cross_contract_authorized_caller_updates_consumer() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let operator = Address::generate(&env);
    let rm_id = env.register(RoleManagerContract, ());
    let rm = RoleManagerContractClient::new(&env, &rm_id);
    rm.initialize(&admin);
    rm.grant_role(&admin, &symbol_short!("operator"), &operator);

    let consumer_id = env.register(ParameterConsumer, ());
    let consumer = ParameterConsumerClient::new(&env, &consumer_id);
    consumer.init(&rm_id);

    consumer.update_value(&operator, &42);
    assert_eq!(consumer.get_value(), 42);
}

#[test]
fn cross_contract_unauthorized_caller_is_rejected() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let outsider = Address::generate(&env);
    let rm_id = env.register(RoleManagerContract, ());
    let rm = RoleManagerContractClient::new(&env, &rm_id);
    rm.initialize(&admin);

    let consumer_id = env.register(ParameterConsumer, ());
    let consumer = ParameterConsumerClient::new(&env, &consumer_id);
    consumer.init(&rm_id);

    let res = consumer.try_update_value(&outsider, &99);
    assert_eq!(res, Err(Ok(RoleManagerError::MissingRole)));
    assert_eq!(consumer.get_value(), 0);
}

#[test]
fn cross_contract_mock_role_manager_denial_propagates() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let caller = Address::generate(&env);
    let mock_id = env.register(MockRoleManager, ());

    let consumer_id = env.register(ParameterConsumer, ());
    let consumer = ParameterConsumerClient::new(&env, &consumer_id);
    consumer.init(&mock_id);

    let res = consumer.try_update_value(&caller, &7);
    assert_eq!(res, Err(Ok(RoleManagerError::MissingRole)));
    assert_eq!(consumer.get_value(), 0);
}
