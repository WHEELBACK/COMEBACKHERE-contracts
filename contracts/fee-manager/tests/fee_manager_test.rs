#![cfg(test)]
extern crate std;

use fee_manager::{FeeManagerContract, FeeManagerContractClient, FeeManagerError};
use soroban_sdk::{
    testutils::Address as _,
    Address, Env,
};

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Register the contract, initialize it with 500 bps (5%), and return the
/// relevant handles. `env.mock_all_auths()` is called so auth checks pass
/// without needing real signatures in tests.
fn setup() -> (Env, Address, Address, FeeManagerContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    let contract_id = env.register(FeeManagerContract, ());
    let client = FeeManagerContractClient::new(&env, &contract_id);
    client.initialize(&admin, &fee_recipient, &500u32);
    (env, admin, fee_recipient, client)
}

// ── Initialization ────────────────────────────────────────────────────────────

#[test]
fn test_initialize_sets_state() {
    let (_env, _admin, _fee_recipient, client) = setup();
    assert_eq!(client.get_fee_bps(), 500u32);
}

#[test]
fn test_double_initialize_fails() {
    let (env, _admin, _fee_recipient, client) = setup();
    let new_admin = Address::generate(&env);
    let new_recipient = Address::generate(&env);
    let result = client.try_initialize(&new_admin, &new_recipient, &100u32);
    assert_eq!(result, Err(Ok(FeeManagerError::AlreadyInitialized)));
}

#[test]
fn test_invalid_fee_bps_on_init() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    let contract_id = env.register(FeeManagerContract, ());
    let client = FeeManagerContractClient::new(&env, &contract_id);
    // 10_001 bps is out of range
    let result = client.try_initialize(&admin, &fee_recipient, &10_001u32);
    assert_eq!(result, Err(Ok(FeeManagerError::InvalidFeeBps)));
}

// ── set_fee_bps ───────────────────────────────────────────────────────────────

#[test]
fn test_set_fee_bps_admin() {
    let (_env, admin, _fee_recipient, client) = setup();
    client.set_fee_bps(&admin, &300u32);
    assert_eq!(client.get_fee_bps(), 300u32);
}

#[test]
fn test_set_fee_bps_operator() {
    let (env, admin, _fee_recipient, client) = setup();
    let operator = Address::generate(&env);
    client.add_operator(&admin, &operator);
    client.set_fee_bps(&operator, &200u32);
    assert_eq!(client.get_fee_bps(), 200u32);
}

#[test]
fn test_set_fee_bps_unauthorized() {
    let (env, _admin, _fee_recipient, client) = setup();
    let attacker = Address::generate(&env);
    let result = client.try_set_fee_bps(&attacker, &100u32);
    assert_eq!(result, Err(Ok(FeeManagerError::Unauthorized)));
}

#[test]
fn test_invalid_fee_bps_on_set() {
    let (_env, admin, _fee_recipient, client) = setup();
    let result = client.try_set_fee_bps(&admin, &10_001u32);
    assert_eq!(result, Err(Ok(FeeManagerError::InvalidFeeBps)));
}

// ── set_fee_recipient ─────────────────────────────────────────────────────────

#[test]
fn test_set_fee_recipient_admin() {
    let (env, admin, _fee_recipient, client) = setup();
    let new_recipient = Address::generate(&env);
    client.set_fee_recipient(&admin, &new_recipient);
    assert_eq!(client.get_fee_recipient(), new_recipient);
}

#[test]
fn test_set_fee_recipient_unauthorized() {
    let (env, _admin, fee_recipient, client) = setup();
    let attacker = Address::generate(&env);
    let new_recipient = Address::generate(&env);
    let result = client.try_set_fee_recipient(&attacker, &new_recipient);
    assert_eq!(result, Err(Ok(FeeManagerError::Unauthorized)));
    // fee_recipient unchanged
    assert_eq!(client.get_fee_recipient(), fee_recipient);
}

// ── compute_fee ───────────────────────────────────────────────────────────────

#[test]
fn test_compute_fee_basic() {
    // 500 bps = 5% of 1_000_000 = 50_000 fee, 950_000 net
    let (_env, _admin, _fee_recipient, client) = setup();
    let (fee, net) = client.compute_fee(&1_000_000i128);
    assert_eq!(fee, 50_000i128);
    assert_eq!(net, 950_000i128);
}

#[test]
fn test_compute_fee_zero_bps() {
    let (env, admin, fee_recipient, _) = setup();
    let contract_id = env.register(FeeManagerContract, ());
    let client = FeeManagerContractClient::new(&env, &contract_id);
    client.initialize(&admin, &fee_recipient, &0u32);
    let (fee, net) = client.compute_fee(&1_000_000i128);
    assert_eq!(fee, 0i128);
    assert_eq!(net, 1_000_000i128);
}

#[test]
fn test_compute_fee_full_bps() {
    let (env, admin, fee_recipient, _) = setup();
    let contract_id = env.register(FeeManagerContract, ());
    let client = FeeManagerContractClient::new(&env, &contract_id);
    client.initialize(&admin, &fee_recipient, &10_000u32);
    let (fee, net) = client.compute_fee(&1_000_000i128);
    assert_eq!(fee, 1_000_000i128);
    assert_eq!(net, 0i128);
}

#[test]
fn test_compute_fee_invalid_amount() {
    let (_env, _admin, _fee_recipient, client) = setup();
    let result = client.try_compute_fee(&0i128);
    assert_eq!(result, Err(Ok(FeeManagerError::InvalidAmount)));
    let result_neg = client.try_compute_fee(&-1i128);
    assert_eq!(result_neg, Err(Ok(FeeManagerError::InvalidAmount)));
}

// ── compute_partial_fee ───────────────────────────────────────────────────────

#[test]
fn test_compute_partial_fee_basic() {
    // settled == total should equal compute_fee result
    let (_env, _admin, _fee_recipient, client) = setup();
    let (p_fee, p_net) = client.compute_partial_fee(&1_000_000i128, &1_000_000i128);
    let (fee, net) = client.compute_fee(&1_000_000i128);
    assert_eq!(p_fee, fee);
    assert_eq!(p_net, net);
}

#[test]
fn test_compute_partial_fee_underflow() {
    // settled > total must return ArithmeticUnderflow (#896)
    let (_env, _admin, _fee_recipient, client) = setup();
    let result = client.try_compute_partial_fee(&500_000i128, &1_000_000i128);
    assert_eq!(result, Err(Ok(FeeManagerError::ArithmeticUnderflow)));
}

// ── Pause / unpause ───────────────────────────────────────────────────────────

#[test]
fn test_pause_blocks_mutation() {
    let (_env, admin, _fee_recipient, client) = setup();
    client.pause(&admin);
    let result = client.try_set_fee_bps(&admin, &100u32);
    assert_eq!(result, Err(Ok(FeeManagerError::ContractPaused)));
}

// ── Operator management ───────────────────────────────────────────────────────

#[test]
fn test_add_remove_operator() {
    let (env, admin, _fee_recipient, client) = setup();
    let operator = Address::generate(&env);
    // Not an operator before add_operator
    assert!(!client.is_operator(&operator));
    client.add_operator(&admin, &operator);
    assert!(client.is_operator(&operator));
    client.remove_operator(&admin, &operator);
    assert!(!client.is_operator(&operator));
}

// ── Admin transfer ────────────────────────────────────────────────────────────

#[test]
fn test_transfer_accept_admin() {
    let (env, admin, _fee_recipient, client) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);
    // new admin can now set fee_bps
    client.set_fee_bps(&new_admin, &250u32);
    assert_eq!(client.get_fee_bps(), 250u32);
    // old admin can no longer set fee_bps
    let result = client.try_set_fee_bps(&admin, &100u32);
    assert_eq!(result, Err(Ok(FeeManagerError::Unauthorized)));
}

#[test]
fn test_accept_admin_wrong_caller() {
    let (env, admin, _fee_recipient, client) = setup();
    let new_admin = Address::generate(&env);
    let impostor = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    let result = client.try_accept_admin(&impostor);
    assert_eq!(result, Err(Ok(FeeManagerError::Unauthorized)));
}
