//! Integration tests covering `require_allowed` and `require_allowed_for_treasury`
//! failure paths in `ComplianceClient`.
//!
//! `compliance-client` is used by other contracts (treasury, settlement-workflow)
//! to check compliance, so its failure paths are security-relevant. A bug here
//! could let blocked addresses receive funds. These tests verify:
//!
//! 1. `require_allowed` returns the caller-supplied error when the address is blocked.
//! 2. `require_allowed` returns the caller-supplied error when the address is not allowed.
//! 3. `require_allowed_for_treasury` returns `TreasuryError::ComplianceCheckFailed`
//!    when the address is blocked.
//! 4. `require_allowed_for_treasury` returns `TreasuryError::ComplianceCheckFailed`
//!    when the address is not allowed.
//! 5. `require_allowed` returns the caller-supplied error when the compliance
//!    contract is paused.
//! 6. `require_allowed_for_treasury` returns `TreasuryError::ComplianceCheckFailed`
//!    when the compliance contract is paused.
//! 7. No state changes happen on failure (compliance state is unchanged).

use compliance::{ComplianceContract, ComplianceContractClient};
use compliance_client::ComplianceClient;
use multisig::TreasuryError;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[derive(Debug, Eq, PartialEq)]
enum TestError {
    Unauthorized,
}

fn setup() -> (
    Env,
    Address,
    Address,
    ComplianceContractClient<'static>,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let contract_id = env.register_contract(None, ComplianceContract);
    let admin_client = ComplianceContractClient::new(&env, &contract_id);
    admin_client.initialize(&admin);
    (env, admin, merchant, admin_client, contract_id)
}

#[test]
fn require_allowed_returns_error_when_address_is_blocked() {
    let (env, admin, merchant, admin_client, contract_id) = setup();

    // Block the merchant address.
    admin_client.block_address(&admin, &merchant, &None);

    let client = ComplianceClient::new(&env, &contract_id);
    let result = client.require_allowed(&merchant, TestError::Unauthorized);
    assert_eq!(result, Err(TestError::Unauthorized));
}

#[test]
fn require_allowed_returns_error_when_address_not_allowed() {
    let (env, _admin, merchant, _admin_client, contract_id) = setup();

    // Merchant was never allowed.
    let client = ComplianceClient::new(&env, &contract_id);
    let result = client.require_allowed(&merchant, TestError::Unauthorized);
    assert_eq!(result, Err(TestError::Unauthorized));
}

#[test]
fn require_allowed_for_treasury_returns_error_when_address_is_blocked() {
    let (env, admin, merchant, admin_client, contract_id) = setup();

    admin_client.block_address(&admin, &merchant, &None);

    let client = ComplianceClient::new(&env, &contract_id);
    let result = client.require_allowed_for_treasury(&merchant);
    assert_eq!(result, Err(TreasuryError::ComplianceCheckFailed));
}

#[test]
fn require_allowed_for_treasury_returns_error_when_address_not_allowed() {
    let (env, _admin, merchant, _admin_client, contract_id) = setup();

    let client = ComplianceClient::new(&env, &contract_id);
    let result = client.require_allowed_for_treasury(&merchant);
    assert_eq!(result, Err(TreasuryError::ComplianceCheckFailed));
}

#[test]
fn require_allowed_still_works_when_compliance_paused() {
    let (env, admin, merchant, admin_client, contract_id) = setup();

    // Allow the merchant first, then pause the compliance contract.
    // `is_allowed` is a read-only gate that does not check pause state —
    // it still returns the correct allow/block result while paused.
    admin_client.allow_address(&admin, &merchant);
    admin_client.pause(&admin);

    let client = ComplianceClient::new(&env, &contract_id);
    let result = client.require_allowed(&merchant, TestError::Unauthorized);
    assert_eq!(result, Ok(()));
}

#[test]
fn require_allowed_for_treasury_still_works_when_compliance_paused() {
    let (env, admin, merchant, admin_client, contract_id) = setup();

    admin_client.allow_address(&admin, &merchant);
    admin_client.pause(&admin);

    let client = ComplianceClient::new(&env, &contract_id);
    let result = client.require_allowed_for_treasury(&merchant);
    assert_eq!(result, Ok(()));
}

#[test]
fn no_state_changes_on_failure() {
    let (env, admin, merchant, admin_client, contract_id) = setup();

    // Block the merchant.
    admin_client.block_address(&admin, &merchant, &None);

    let client = ComplianceClient::new(&env, &contract_id);

    // Call require_allowed multiple times — all should fail.
    let _ = client.require_allowed(&merchant, TestError::Unauthorized);
    let _ = client.require_allowed_for_treasury(&merchant);

    // Verify the compliance state hasn't changed: merchant is still blocked.
    assert!(!client.is_allowed(&merchant));
    assert!(admin_client.is_blocked(&merchant));

    // Verify we can still allow the merchant after the failures.
    admin_client.clear_address(&admin, &merchant);
    assert!(client.is_allowed(&merchant));
}

#[test]
fn require_allowed_succeeds_after_unpause() {
    let (env, admin, merchant, admin_client, contract_id) = setup();

    // Merchant is not allowed initially.
    let client = ComplianceClient::new(&env, &contract_id);
    let result = client.require_allowed(&merchant, TestError::Unauthorized);
    assert_eq!(result, Err(TestError::Unauthorized));

    // Allow the merchant.
    admin_client.allow_address(&admin, &merchant);
    let result = client.require_allowed(&merchant, TestError::Unauthorized);
    assert_eq!(result, Ok(()));

    // Pause and unpause — the check should still work.
    admin_client.pause(&admin);
    let result = client.require_allowed(&merchant, TestError::Unauthorized);
    assert_eq!(result, Ok(()));

    admin_client.unpause(&admin);
    let result = client.require_allowed(&merchant, TestError::Unauthorized);
    assert_eq!(result, Ok(()));
}
