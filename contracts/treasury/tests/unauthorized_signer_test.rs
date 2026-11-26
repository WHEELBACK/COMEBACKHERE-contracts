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

// #969 — granular role-based authorization for parameter modification.  Each parameter-mutating
// entrypoint is exercised from the Admin role, the Operator role and an unauthorized signer so
// that a regression in the role matrix surfaces as a failing test rather than a silent bypass.

#[test]
fn admin_can_modify_threshold() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);
    let extra = Address::generate(&env);
    client.set_signer(&admin, &extra, &2);

    client.update_threshold(&admin, &3);
    assert_eq!(client.get_threshold(), 3);
}

#[test]
fn operator_cannot_modify_threshold() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    let operator = Address::generate(&env);
    client.set_signer(&admin, &operator, &2);

    assert!(client.try_update_threshold(&operator, &3).is_err());
}

#[test]
fn unauthorized_signer_cannot_modify_threshold() {
    let env = Env::default();
    let (client, _admin, _contract_id) = setup_treasury(&env, 2);

    let unauthorized = Address::generate(&env);
    assert!(client.try_update_threshold(&unauthorized, &3).is_err());
}

#[test]
fn merchant_can_set_payout_address() {
    let env = Env::default();
    let (client, _admin, _contract_id) = setup_treasury(&env, 2);

    let merchant = Address::generate(&env);
    let payout = Address::generate(&env);
    client.update_merchant_payout_address(&merchant, &payout);
    assert_eq!(client.get_merchant_payout_address(&merchant), Some(payout));
}

#[test]
fn admin_can_set_withdrawal_limit_and_non_admin_cannot() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    let operator = Address::generate(&env);
    client.set_signer(&admin, &operator, &1);

    client.set_withdrawal_limit(&admin, &5_000_000, &3600);
    assert!(client
        .try_set_withdrawal_limit(&operator, &1_000_000, &3600)
        .is_err());

    let unauthorized = Address::generate(&env);
    assert!(client
        .try_set_withdrawal_limit(&unauthorized, &1_000_000, &3600)
        .is_err());
}

#[test]
fn admin_can_force_cancel_settlement() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    client.force_cancel_settlement(&admin, &settlement_id);
    assert_eq!(
        client.get_settlement(&settlement_id).status,
        treasury::SettlementStatus::Cancelled
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn operator_cannot_force_cancel_settlement() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    let operator = Address::generate(&env);
    client.set_signer(&admin, &operator, &1);

    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Force-cancel is an admin-only escape hatch.
    client.force_cancel_settlement(&operator, &settlement_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn unauthorized_signer_cannot_force_cancel_settlement() {
    let env = Env::default();
    let (client, admin, _contract_id) = setup_treasury(&env, 2);

    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    let unauthorized = Address::generate(&env);
    client.force_cancel_settlement(&unauthorized, &settlement_id);
}
