use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

fn setup_treasury(env: &Env, threshold: u32) -> (TreasuryContractClient<'_>, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &threshold, &soroban_sdk::Vec::new(env));
    (client, admin, contract_id)
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn unauthorized_signer_cannot_propose_settlement() {
    let env = Env::default();
    let (client, _admin, _contract_id) = setup_treasury(&env, 2);

    // Generate an address that is not registered as a signer
    let unauthorized = Address::generate(&env);
    let merchant = Address::generate(&env);

    // This should panic with "UnauthorizedSigner" because unauthorized has weight 0
    client.propose_settlement(&unauthorized, &merchant, &10_000_000, &0_u64);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn unauthorized_signer_cannot_approve_settlement() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    // Create a settlement with the admin (who is authorized)
    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Try to approve with an unauthorized signer
    let unauthorized = Address::generate(&env);

    // This should panic with "UnauthorizedSigner"
    client.approve_settlement(&unauthorized, &settlement_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn unauthorized_signer_cannot_approve_partial_settlement() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    // Create a settlement with the admin
    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Try to approve partial settlement with an unauthorized signer
    let unauthorized = Address::generate(&env);

    // This should panic with "UnauthorizedSigner"
    client.approve_partial_settlement(&unauthorized, &settlement_id, &5_000_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn unauthorized_signer_cannot_execute_settlement() {
    let env = Env::default();
    let (client, admin, contract_id) = setup_treasury(&env, 1);

    // Create and approve a settlement
    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Try to execute with an unauthorized signer
    let unauthorized = Address::generate(&env);
    let token_contract = Address::generate(&env);

    // This should panic with "UnauthorizedSigner"
    client.execute_settlement(&unauthorized, &settlement_id, &token_contract);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn unauthorized_signer_cannot_cancel_settlement() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    // Create a settlement
    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Try to cancel with an unauthorized signer
    let unauthorized = Address::generate(&env);

    // This should panic with "UnauthorizedSigner"
    client.cancel_settlement(&unauthorized, &settlement_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn unauthorized_signer_cannot_partially_execute_settlement() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 1);

    // Create a settlement
    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Try to partially execute with an unauthorized signer
    let unauthorized = Address::generate(&env);
    let token_contract = Address::generate(&env);

    // This should panic with "UnauthorizedSigner"
    client.partially_execute_settlement(&unauthorized, &settlement_id, &5_000_000, &token_contract);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn unauthorized_signer_cannot_vote_on_dispute_resolution() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    // Create a settlement and raise a dispute
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let dispute_id = client.raise_dispute(
        &claimant,
        &settlement_id,
        &merchant,
        &5_000_000,
        &u64::MAX,
        &None,
    );

    // Try to vote with an unauthorized signer
    let unauthorized = Address::generate(&env);

    // This should panic with "UnauthorizedSigner"
    client.vote_dispute_resolution(&unauthorized, &dispute_id, &true);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn unauthorized_signer_cannot_propose_signer_rotation() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    // Try to propose rotation with an unauthorized signer
    let unauthorized = Address::generate(&env);
    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);

    // This should panic with "UnauthorizedSigner"
    client.propose_signer_rotation(&unauthorized, &old_signer, &new_signer);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn unauthorized_signer_cannot_approve_signer_rotation() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    // Create a rotation proposal with admin
    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let rotation_id = client.propose_signer_rotation(&admin, &old_signer, &new_signer);

    // Try to approve with an unauthorized signer
    let unauthorized = Address::generate(&env);

    // This should panic with "UnauthorizedSigner"
    client.approve_signer_rotation(&unauthorized, &rotation_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn signer_with_zero_weight_is_unauthorized() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    // Create a signer and then set their weight to 0
    let former_signer = Address::generate(&env);
    client.set_signer(&admin, &former_signer, &1);

    // Verify they can propose when authorized
    let merchant = Address::generate(&env);
    let _settlement_id = client.propose_settlement(&former_signer, &merchant, &10_000_000, &0_u64);

    // Now remove their authorization by setting weight to 0
    client.set_signer(&admin, &former_signer, &0);

    // This should now panic with "UnauthorizedSigner"
    client.propose_settlement(&former_signer, &merchant, &20_000_000, &0_u64);
}

#[test]
fn authorized_signer_can_perform_operations() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    // Add a second authorized signer
    let authorized_signer = Address::generate(&env);
    client.set_signer(&admin, &authorized_signer, &1);

    // Verify authorized signer can propose settlement
    let merchant = Address::generate(&env);
    let settlement_id =
        client.propose_settlement(&authorized_signer, &merchant, &10_000_000, &0_u64);
    assert_eq!(settlement_id, 1);

    // Verify authorized signer can approve settlement
    let settlement = client.approve_settlement(&admin, &settlement_id);
    assert_eq!(settlement.approvals.len(), 2);

    // Verify authorized signer can propose rotation
    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let rotation_id = client.propose_signer_rotation(&authorized_signer, &old_signer, &new_signer);
    assert_eq!(rotation_id, 1);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn unauthorized_signer_cannot_propose_partial_settlement() {
    let env = Env::default();
    let (client, _admin, _contract_id) = setup_treasury(&env, 2);

    // Generate an unauthorized address
    let unauthorized = Address::generate(&env);
    let merchant = Address::generate(&env);

    // This should panic with "UnauthorizedSigner"
    client.propose_partial_settlement(&unauthorized, &merchant, &5_000_000, &0_u64);
}

#[test]
fn admin_is_automatically_authorized_signer() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 1);

    // Admin should be able to propose settlements without explicit set_signer call
    // because initialize sets admin as a signer with weight 1
    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    assert_eq!(settlement_id, 1);
}

// #36 — comprehensive guard table: every entrypoint protected by require_authorized_signer must
// reject a non-signer address.  The five tests below cover the five required entrypoints so that
// adding a new entrypoint without the guard will surface as a missing entry here.

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn guard_table_propose_settlement_rejects_unauthorized() {
    let env = Env::default();
    let (client, _admin, _) = setup_treasury(&env, 2);
    let merchant = Address::generate(&env);
    client.propose_settlement(&Address::generate(&env), &merchant, &1_000_000, &0_u64);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn guard_table_approve_settlement_rejects_unauthorized() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);
    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let unauthorized = Address::generate(&env);
    client.approve_settlement(&unauthorized, &settlement_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn guard_table_execute_settlement_rejects_unauthorized() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 1);
    let merchant = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let token_contract = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    client.execute_settlement(&unauthorized, &settlement_id, &token_contract);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn guard_table_propose_signer_rotation_rejects_unauthorized() {
    let env = Env::default();
    let (client, _admin, _) = setup_treasury(&env, 2);
    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    client.propose_signer_rotation(&unauthorized, &old_signer, &new_signer);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn guard_table_approve_signer_rotation_rejects_unauthorized() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);
    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let rotation_id = client.propose_signer_rotation(&admin, &old_signer, &new_signer);
    client.approve_signer_rotation(&unauthorized, &rotation_id);
}

// #960 — granular role-based authorization for parameter-mutation entrypoints.
//
// Every parameter-mutation entrypoint must reject callers that do not hold the required role
// with `RoleUnauthorized` (contract error #11) and must leave storage untouched.  The tests
// below cover three classes of malicious caller:
//   1. an address that was never registered as a signer,
//   2. a registered signer that lacks the required role,
//   3. a signer whose role was revoked after being granted.
// Each test also asserts that the mutated parameter is unchanged after the rejected call.

use soroban_sdk::testutils::Ledger as _;

const ROLE_UNAUTHORIZED: u32 = 11;

fn setup_roles(env: &Env) -> (TreasuryContractClient<'_>, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &2_u32, &soroban_sdk::Vec::new(env));
    (client, admin, contract_id)
}

/// Registers `signer` with weight 1 and no privileged roles.
fn register_plain_signer(client: &TreasuryContractClient<'_>, admin: &Address, signer: &Address) {
    client.set_signer(admin, signer, &1_u32);
}

#[test]
fn unauthorized_address_cannot_change_threshold() {
    let env = Env::default();
    let (client, _admin, _contract_id) = setup_roles(&env);
    let attacker = Address::generate(&env);

    let result = client.try_set_threshold(&attacker, &5_u32);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_threshold(), 2_u32);
}

#[test]
fn registered_signer_without_role_cannot_change_threshold() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);

    let result = client.try_set_threshold(&signer, &5_u32);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_threshold(), 2_u32);
}

#[test]
fn revoked_role_caller_cannot_change_threshold() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);
    client.grant_role(&admin, &signer, &treasury::Role::Admin);

    // Sanity check: the granted role allows the mutation.
    client.set_threshold(&signer, &3_u32);
    assert_eq!(client.get_threshold(), 3_u32);

    // Revoke the role and verify the caller is rejected and storage is unchanged.
    client.revoke_role(&admin, &signer, &treasury::Role::Admin);
    let result = client.try_set_threshold(&signer, &5_u32);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_threshold(), 3_u32);
}

#[test]
fn unauthorized_address_cannot_change_signer_weight() {
    let env = Env::default();
    let (client, _admin, _contract_id) = setup_roles(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let result = client.try_set_signer(&attacker, &target, &9_u32);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_signer_weight(&target), 0_u32);
}

#[test]
fn registered_signer_without_role_cannot_change_signer_weight() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    let target = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);

    let result = client.try_set_signer(&signer, &target, &9_u32);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_signer_weight(&target), 0_u32);
}

#[test]
fn revoked_role_caller_cannot_change_signer_weight() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    let target = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);
    client.grant_role(&admin, &signer, &treasury::Role::Admin);

    client.set_signer(&signer, &target, &4_u32);
    assert_eq!(client.get_signer_weight(&target), 4_u32);

    client.revoke_role(&admin, &signer, &treasury::Role::Admin);
    let result = client.try_set_signer(&signer, &target, &9_u32);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_signer_weight(&target), 4_u32);
}

#[test]
fn unauthorized_address_cannot_change_label() {
    let env = Env::default();
    let (client, _admin, _contract_id) = setup_roles(&env);
    let attacker = Address::generate(&env);
    let label = soroban_sdk::String::from_str(&env, "hijacked");

    let result = client.try_set_label(&attacker, &label);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_ne!(client.get_label(), label);
}

#[test]
fn registered_signer_without_role_cannot_change_label() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);
    let label = soroban_sdk::String::from_str(&env, "hijacked");

    let result = client.try_set_label(&signer, &label);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_ne!(client.get_label(), label);
}

#[test]
fn revoked_role_caller_cannot_change_label() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);
    client.grant_role(&admin, &signer, &treasury::Role::Admin);

    let original = soroban_sdk::String::from_str(&env, "original");
    client.set_label(&signer, &original);
    assert_eq!(client.get_label(), original);

    client.revoke_role(&admin, &signer, &treasury::Role::Admin);
    let hijacked = soroban_sdk::String::from_str(&env, "hijacked");
    let result = client.try_set_label(&signer, &hijacked);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_label(), original);
}

#[test]
fn unauthorized_address_cannot_change_compliance_id() {
    let env = Env::default();
    let (client, _admin, _contract_id) = setup_roles(&env);
    let attacker = Address::generate(&env);
    let compliance_id = soroban_sdk::String::from_str(&env, "evil-compliance");

    let result = client.try_set_compliance_id(&attacker, &compliance_id);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_ne!(client.get_compliance_id(), compliance_id);
}

#[test]
fn registered_signer_without_role_cannot_change_compliance_id() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);
    let compliance_id = soroban_sdk::String::from_str(&env, "evil-compliance");

    let result = client.try_set_compliance_id(&signer, &compliance_id);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_ne!(client.get_compliance_id(), compliance_id);
}

#[test]
fn revoked_role_caller_cannot_change_compliance_id() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);
    client.grant_role(&admin, &signer, &treasury::Role::Admin);

    let original = soroban_sdk::String::from_str(&env, "original-compliance");
    client.set_compliance_id(&signer, &original);
    assert_eq!(client.get_compliance_id(), original);

    client.revoke_role(&admin, &signer, &treasury::Role::Admin);
    let hijacked = soroban_sdk::String::from_str(&env, "evil-compliance");
    let result = client.try_set_compliance_id(&signer, &hijacked);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_compliance_id(), original);
}

#[test]
fn unauthorized_address_cannot_transfer_admin() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let attacker = Address::generate(&env);
    let new_admin = Address::generate(&env);

    let result = client.try_transfer_admin(&attacker, &new_admin);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn registered_signer_without_role_cannot_transfer_admin() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    let new_admin = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);

    let result = client.try_transfer_admin(&signer, &new_admin);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn revoked_role_caller_cannot_transfer_admin() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    let new_admin = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);
    client.grant_role(&admin, &signer, &treasury::Role::Admin);

    client.revoke_role(&admin, &signer, &treasury::Role::Admin);
    let result = client.try_transfer_admin(&signer, &new_admin);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn unauthorized_address_cannot_place_hold() {
    let env = Env::default();
    let (client, _admin, _contract_id) = setup_roles(&env);
    let attacker = Address::generate(&env);
    let subject = Address::generate(&env);

    let result = client.try_place_hold(&attacker, &subject, &1_000_000_i128);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_held_amount(&subject), 0_i128);
}

#[test]
fn registered_signer_without_role_cannot_place_hold() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    let subject = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);

    let result = client.try_place_hold(&signer, &subject, &1_000_000_i128);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_held_amount(&subject), 0_i128);
}

#[test]
fn revoked_role_caller_cannot_place_hold() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    let subject = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);
    client.grant_role(&admin, &signer, &treasury::Role::Admin);

    client.place_hold(&signer, &subject, &1_000_000_i128);
    assert_eq!(client.get_held_amount(&subject), 1_000_000_i128);

    client.revoke_role(&admin, &signer, &treasury::Role::Admin);
    let result = client.try_place_hold(&signer, &subject, &2_000_000_i128);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_held_amount(&subject), 1_000_000_i128);
}

#[test]
fn unauthorized_address_cannot_release_hold() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let attacker = Address::generate(&env);
    let subject = Address::generate(&env);
    client.place_hold(&admin, &subject, &1_000_000_i128);

    let result = client.try_release_hold(&attacker, &subject);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_held_amount(&subject), 1_000_000_i128);
}

#[test]
fn registered_signer_without_role_cannot_release_hold() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    let subject = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);
    client.place_hold(&admin, &subject, &1_000_000_i128);

    let result = client.try_release_hold(&signer, &subject);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_held_amount(&subject), 1_000_000_i128);
}

#[test]
fn revoked_role_caller_cannot_release_hold() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let signer = Address::generate(&env);
    let subject = Address::generate(&env);
    register_plain_signer(&client, &admin, &signer);
    client.grant_role(&admin, &signer, &treasury::Role::Admin);
    client.place_hold(&admin, &subject, &1_000_000_i128);

    client.release_hold(&signer, &subject);
    assert_eq!(client.get_held_amount(&subject), 0_i128);

    // Re-place the hold as admin, then verify the revoked signer cannot release it.
    client.place_hold(&admin, &subject, &1_000_000_i128);
    client.revoke_role(&admin, &signer, &treasury::Role::Admin);
    let result = client.try_release_hold(&signer, &subject);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_held_amount(&subject), 1_000_000_i128);
}

#[test]
fn spoofed_auth_cannot_mutate_parameters() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let attacker = Address::generate(&env);

    // The attacker attempts to spoof the admin's authorization by mocking auths for the
    // attacker only; the contract must still reject the call because the attacker holds no role.
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_threshold",
            args: (attacker.clone(), 5_u32).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_set_threshold(&attacker, &5_u32);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_threshold(), 2_u32);

    // Restore blanket auth mocking for the remaining assertions.
    env.mock_all_auths();
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn spoofed_auth_cannot_transfer_admin() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_roles(&env);
    let attacker = Address::generate(&env);
    let new_admin = Address::generate(&env);

    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "transfer_admin",
            args: (attacker.clone(), new_admin.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_transfer_admin(&attacker, &new_admin);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(client.get_admin(), admin);

    env.mock_all_auths();
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn rejected_mutation_does_not_consume_ledger_entries() {
    let env = Env::default();
    let (client, _admin, _contract_id) = setup_roles(&env);
    let attacker = Address::generate(&env);

    // Snapshot the ledger sequence before the rejected call; a rejected mutation must not
    // advance contract state or bump the ledger.
    let before = env.ledger().sequence();
    let result = client.try_set_threshold(&attacker, &5_u32);
    assert_eq!(result, Err(Ok(ROLE_UNAUTHORIZED)));
    assert_eq!(env.ledger().sequence(), before);
    assert_eq!(client.get_threshold(), 2_u32);
}
